<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import Icon from './Icon.svelte';
  import Avatar from './Avatar.svelte';
  import ProfileSettings from './ProfileSettings.svelte';
  import PinsPanel from './PinsPanel.svelte';
  import ThreadList from './ThreadList.svelte';
  import ChannelPermissions from './ChannelPermissions.svelte';
  import CustomEmojiSettings from './CustomEmojiSettings.svelte';
  import EmojiGlyph from './EmojiGlyph.svelte';
  import { memberPresence } from './lib/profile';
  import { emojiGroups, customReaction, customToken, reactionName } from './lib/custom-emoji';
  import type { User, CustomEmoji } from './lib/api';
  import Appearance from './Appearance.svelte';import{isDesktopApp,desktopNotify,onDesktopNotificationClick}from'./lib/desktop';import{applyTheme,loadTheme,storeTheme}from'./lib/theme';
  import{prepareNotifications,knownNotificationTarget,enablePush,disablePush,type NotificationTarget}from'./lib/desktop-notifications';
  import SearchPanel from './SearchPanel.svelte';
  import NotificationSettings from './NotificationSettings.svelte';
  import MentionMenu from './MentionMenu.svelte';
  import EmojiPicker from './EmojiPicker.svelte';
  import{defaultPreferences,newer,unread,mentioned,shouldNotify,received,acknowledged,readUpdate,mergeReads,mentionParts,loadDeviceNotifications,storeDeviceNotifications,type DeviceNotifications}from'./lib/social';
  import MessageText from './MessageText.svelte';
  import AiReview from './AiReview.svelte';
  import { noticeKey, noticeSignature, readPreference, writePreference } from './lib/panels';
  import { coversChannel, mergeFlags } from './lib/ai';
  import DiscoveryBrowser from './DiscoveryBrowser.svelte';
  import AdminPanel from './AdminPanel.svelte';
  import MemberControls from './MemberControls.svelte';
  import SavedServers from './SavedServers.svelte';
  import HostServers from './HostServers.svelte';
  import{loadSavedServers,storeSavedServers,rememberServer,serverIdentity,parseServerLink,type SavedServer,type ServerTarget}from'./lib/saved-servers';
  import PendingAttachments from './PendingAttachments.svelte';
  import VoiceRecorder from './VoiceRecorder.svelte';
  import UploadTray from './UploadTray.svelte';
  import MessageMedia from './MessageMedia.svelte';
  import VoicePanel from './VoicePanel.svelte';
  import { VoiceWakeLock } from './lib/wake-lock';
  import { VoiceClient, secureVoiceAvailable, roomUploadMbps } from './lib/voice';
  import { fileInput, mediaUrl } from './lib/media';
  import { Api, ApiError, can, deviceId, normalizeHost, sessionKey } from './lib/api';
  import type { DirectMessage,ReadState,NotificationPreferences,HostInfo, ServerSummary, Snapshot, Channel, Message, Category, Member, Role, InvitePreview, Attachment, VoiceState, AiFlag } from './lib/api';
  import { LiveConnection } from './lib/connection';
  import type { ConnectionStatus, Envelope } from './lib/connection';
  import { mergeMessages, mergeEventMessage, mergeHistory, updateReaction, upsert } from './lib/state';
  import { contentLimits, sendRestriction } from './lib/limits';
  import { readSession, saveSession, clearSession } from './lib/session';
  import type { HostServer, CreatedServer } from './lib/host-servers';
  import { randomId } from './lib/random';

  let origin = location.origin;
  let hostInput = origin;
  let savedServers=loadSavedServers();
  let themeColor=loadTheme();
  $: linkTarget = pendingLink ? parseServerLink(pendingLink) : null;
  function savedTheme(color:string|null){themeColor=color;applyTheme(color);storeTheme(color);}
  let host: HostInfo | null = null;
  let selected: ServerSummary | null = null;
  let api = new Api(origin);
  let snapshot: Snapshot | null = null;
  let connection: LiveConnection | undefined;
  const voice = new VoiceClient((type, data) => connection?.send(type, data) ?? false);
  const voiceStatus = voice.state;
  const voiceWakeLock = new VoiceWakeLock(navigator.wakeLock, document);
  let keepScreenOn=readPreference('chitchat.screen-awake')!=='0';
  $: voiceWakeLock.setActive(keepScreenOn && !!snapshot);
  function setKeepScreenOn(value:boolean){keepScreenOn=value;writePreference('chitchat.screen-awake',value?'1':'0');}
  let voiceStates: VoiceState[] = [];
  let voiceSpeaking: Record<string, boolean> = {};
  const voiceAvailable = secureVoiceAvailable();
  const channelViews = new Map<string, { messages: Message[]; hasOlder: boolean; scrollTop: number; atBottom: boolean }>();
  let status: ConnectionStatus = 'offline';
  let modal: 'host' | 'servers' | 'auth' | 'channel' | 'delete' | 'link' | 'admin' | 'member' | 'channel-settings' | 'media' | 'ai-review' | 'browse' | 'saved' | 'host-create' | 'search' | 'notifications' | 'dm-picker' | 'appearance' | 'profile' | 'pins' | 'threads' | 'channel-permissions' | 'server-emojis' | null = 'servers';
  let hubHostServers:HostServer[]|null=null;
  async function openServerHub(){error='';modal='saved';hubHostServers=null;const currentApi=api;if(!snapshot)return;try{const servers=await currentApi.request<HostServer[]>('/host/servers');if(api===currentApi)hubHostServers=servers;}catch{/* Host tools are shown only after the host confirms administrator access. */}}
  let aiFlags: AiFlag[] = [], latestAiFlag: AiFlag | null = null, aiMore = false, aiLoading = false;
  let aiRefreshVersion = 0;
  let aiNoticeCollapsed = false;
  $: aiNoticeKey = noticeKey(origin, api.slug, activeId);
  $: aiNoticeSignature = noticeSignature(snapshot?.ai_moderation);
  $: aiNoticeCollapsed = !!aiNoticeSignature && readPreference(aiNoticeKey) === aiNoticeSignature;
  let dmNoticeCollapsed = readPreference('chitchat.dm.notice') === '1';
  function setDmNoticeCollapsed(value: boolean) { writePreference('chitchat.dm.notice', value ? '1' : null); dmNoticeCollapsed = value; }
  function setAiNoticeCollapsed(value: boolean) { writePreference(aiNoticeKey,value ? aiNoticeSignature : null); aiNoticeCollapsed = value; }
  let pendingLink = '';
  let selectedMember: Member | null = null;
  let viewedAttachment: Attachment | null = null;
  let attachments: Attachment[] = [];
  let uploading = false, recording = false;
  let uploadTray: UploadTray;
  let channelSettings: Channel | null = null;
  let confirmChannelDelete = false;
  let invitePreview: InvitePreview | null = null;
  let error = '';
  let chatError = '';
  let busy = false;
  let loading = false;
  let olderBusy = false;
  let hasOlder = false;
  let activeId = '';
  let dms:DirectMessage[]=[],readStates:ReadState[]=[],preferences=defaultPreferences(),deviceNotifications=loadDeviceNotifications();
  let pendingNotificationTarget:NotificationTarget|null=null,stopDesktopClicks=()=>{};
  let preferencesLoaded=false;let dmQuery='';let socialError='',socialLoading=false,socialAbort:AbortController|undefined,preferenceVersion=0;
  let viewingHistory=false,jumpId='',mentionCursor=0,mentionMenu:MentionMenu;
  const appAbort=new AbortController();let appAlive=true;const railReads=new Map<string,{token:string;rows:ReadState[]}>();
  let railBadges:Record<string,{unread:boolean;mentions:number}>={};let railTimer:ReturnType<typeof setInterval>,railPolling=false;
  let readRefreshTimer:ReturnType<typeof setTimeout>|undefined;let ackTimer:ReturnType<typeof setTimeout>|undefined;const ackInFlight=new Map<string,string>();
  let soundAudio:HTMLAudioElement|undefined,soundBlocked=false;const notifiedIds=new Set<string>();

  let messages: Message[] = [];
  let draft = '';
  let reply: Message | null = null;
  let deleting: Message | null = null;
  let reacting: { message: Message; anchor: HTMLElement } | null = null;
  let composeEmojiAnchor: HTMLElement | null = null;
  let pinsPanel: PinsPanel;
  $: customEmojiGroups = emojiGroups(snapshot?.emojis ?? [], origin);
  $: activeParent = snapshot?.channels.find(channel => channel.id === active?.parent_id);
  $: selfMember = snapshot?.members.find(member => member.user.id === snapshot?.user.id);
  let editing: Message | null = null;
  let editText = '';
  let sending = false;
  let actionBusy = false;
  let historyAbort: AbortController | undefined;
  let historyVersion = 0;
  let contextVersion = 0;
  let permissionRefreshVersion = 0;
  let liveChanges = new Map<string, Message | null>();
  let ownReactionEvents = new Map<string, Map<string, boolean>>();
  let cooldownUntil = 0, mutedUntil = 0;
  let keepSignedIn = true;
  let mode: 'login' | 'register' = 'login';
  let username = '', password = '', displayName = '', serverPassword = '', inviteCode = '', setupCode = '';
  let showSetup = false;
  let slugInput = '';
  let channelName = '', channelTopic = '', channelCategory = '';
  let channelType: 'text' | 'voice' = 'text', channelLimit = 10;
  let navOpen = false;
  let showMembers = window.innerWidth > 850;
  let messageScroll: HTMLDivElement;
  let composer: HTMLTextAreaElement;
  let typing: Record<string, number> = {};
  let now = Date.now();
  let clock: ReturnType<typeof setInterval>;
  let toast = '';
  let toastTimer: ReturnType<typeof setTimeout>;

  $: active = snapshot?.channels.find(channel => channel.id === activeId)??(dms.find(dm=>dm.id===activeId)?dmChannel(dms.find(dm=>dm.id===activeId)!):undefined);
  $: currentBadge={unread:readStates.some(unread),mentions:readStates.reduce((sum,row)=>sum+row.mention_count,0)};
  $: sortedDms=[...dms].sort((a,b)=>newer(a.last_message_id,b.last_message_id)?-1:newer(b.last_message_id,a.last_message_id)?1:a.user.display_name.localeCompare(b.user.display_name));
  $: categories = [...(snapshot?.categories ?? [])].sort((a, b) => a.position - b.position);
  $: channels = [...(snapshot?.channels ?? [])].filter(channel => !channel.parent_id).sort((a, b) => a.position - b.position);
  $: onlineMembers = snapshot?.members.filter(member => member.online) ?? [];
  $: sortedMembers = [...(snapshot?.members ?? [])].sort((a, b) => Number(b.online) - Number(a.online) || a.user.display_name.localeCompare(b.user.display_name));
  $: sendAllowed = !!active && active.type !== 'voice' && !active.archived && (active.type==='dm'?can(snapshot?.permissions,16,snapshot?.is_owner):can(active.my_permissions,1,snapshot?.is_owner));
  $: draftLimits = contentLimits(draft, attachments.length);
  $: cooldownSeconds = Math.max(0, Math.ceil((cooldownUntil - now) / 1000));
  $: muted = mutedUntil > now;
  $: manageChannels = can(snapshot?.permissions, 3, snapshot?.is_owner);
  $: settingsAllowed = !!snapshot && [4, 5, 6, 10, 11].some(bit => can(snapshot?.permissions, bit, snapshot?.is_owner));
  $: attachAllowed = !!active && active.type !== 'voice' && active.media_allowed && can(active.my_permissions, 7, snapshot?.is_owner) && sendAllowed;
  $: selfTimeout = snapshot?.members.find(member => member.user.id === snapshot?.user.id)?.timed_out_until;
  $: if (selfTimeout !== undefined) mutedUntil = selfTimeout ? Date.parse(selfTimeout) : 0;
  $: if (snapshot) voice.setCapabilities(can(snapshot.permissions, 13, snapshot.is_owner), can(snapshot.permissions, 14, snapshot.is_owner), muted || !!voiceStates.find(item => item.user_id === snapshot?.user.id)?.suppressed);
  $: typingNames = snapshot?.members.filter(member => typing[member.user.id] > now && member.user.id !== snapshot?.user.id).map(member => member.user.display_name) ?? [];

  function profileSaved(user: User) {
    if (!snapshot) return;
    snapshot = { ...snapshot, user: snapshot.user.id === user.id ? user : snapshot.user, members: snapshot.members.map(member => member.user.id === user.id ? { ...member, user } : member) };
    const update = (message: Message) => message.author.id === user.id ? { ...message, author: { ...message.author, ...user } } : message;
    messages = messages.map(update);
    for (const view of channelViews.values()) view.messages = view.messages.map(update);
    dms = dms.map(dm => dm.user.id === user.id ? { ...dm, user } : dm);
  }
  function updatePin(id: string, pinned: boolean, channelId: string) {
    const update = (message: Message) => message.id === id ? { ...message, pinned } : message;
    if (channelId === activeId) { messages = messages.map(update); const changed = messages.find(message => message.id === id); if (changed) liveChanges.set(id, changed); }
    const view = channelViews.get(channelId); if (view) view.messages = view.messages.map(update);
    if (modal === 'pins' && channelId === activeId) void pinsPanel?.refresh();
  }
  async function togglePin(message: Message) {
    if (actionBusy) return; actionBusy = true;
    const currentApi = api, pinned = !message.pinned;
    try { await currentApi.request(`/channels/${message.channel_id}/pins/${message.id}`, pinned ? 'PUT' : 'DELETE'); if (api === currentApi) updatePin(message.id, pinned, message.channel_id); }
    catch (cause) { if (api === currentApi) handleError(cause); }
    finally { actionBusy = false; }
  }
  async function startThread(message: Message) {
    if (!snapshot || actionBusy) return;
    const existing = snapshot.channels.find(channel => channel.id === message.thread_id);
    if (existing) { await openChannel(existing); return; }
    actionBusy = true; const currentApi = api;
    try { const thread = await currentApi.request<Channel>(`/channels/${message.channel_id}/messages/${message.id}/thread`, 'POST', {}); if (api === currentApi && snapshot) { snapshot = { ...snapshot, channels: upsert(snapshot.channels, thread) }; await openChannel(thread); } }
    catch (cause) { if (api === currentApi) handleError(cause); }
    finally { actionBusy = false; }
  }
  async function jumpThreadRoot() {
    if (!active?.root_message_id || !active.parent_id) return;
    const currentApi = api, root = active.root_message_id, parent = active.parent_id;
    try { const rows = await currentApi.request<Message[]>(`/channels/${parent}/messages?around=${root}&limit=1`); if (api !== currentApi) return; const starter = rows.find(message => message.id === root); if (starter) await jumpToMessage(starter); else notify('The thread starter was removed.'); }
    catch (cause) { if (api === currentApi) handleError(cause); }
  }
  async function composeEmoji(value: string) {
    const custom = snapshot?.emojis?.find(emoji => customReaction(emoji) === value);
    const text = custom ? customToken(custom) : value;
    const start = composer?.selectionStart ?? draft.length, end = composer?.selectionEnd ?? start;
    draft = draft.slice(0, start) + text + draft.slice(end); mentionCursor = start + text.length;
    await tick(); composer?.focus(); composer?.setSelectionRange(mentionCursor, mentionCursor);
  }
  function dmChannel(dm:DirectMessage):Channel{return{id:dm.id,type:'dm',name:dm.user.display_name,topic:'Direct message',position:0,category_id:null,media_allowed:dm.media_allowed??true,my_permissions:dm.my_permissions??snapshot?.permissions??'0'};}
  function stateFor(id:string,states=readStates){return states.find(row=>row.channel_id===id);}
  function selfRoles(){const member=snapshot?.members.find(m=>m.user.id===snapshot?.user.id);return snapshot?.roles.filter(r=>r.is_default||member?.role_ids.includes(r.id)).map(r=>r.id)??[];}
  function reading(channel=activeId){return channel===activeId&&!viewingHistory&&!loading&&modal===null&&document.visibilityState==='visible'&&document.hasFocus()&&!!messageScroll&&messageScroll.scrollHeight-messageScroll.scrollTop-messageScroll.clientHeight<80;}
  async function loadSocial(desired?:string,expectedHistory=historyVersion){if(!snapshot)return;socialAbort?.abort();socialAbort=new AbortController();const signal=socialAbort.signal,target=api,version=contextVersion,prefVersion=preferenceVersion,initialReads=readStates;socialLoading=true;socialError='';const result=await Promise.allSettled([target.request<ReadState[]>('/read-states','GET',undefined,signal),target.request<DirectMessage[]>('/dms','GET',undefined,signal),target.request<NotificationPreferences>('/me/preferences','GET',undefined,signal)]);if(signal.aborted||target!==api||version!==contextVersion)return;const[reads,direct,prefs]=result;if(reads.status==='fulfilled')readStates=readStates===initialReads?reads.value:mergeReads(readStates,reads.value);if(direct.status==='fulfilled')dms=[...new Map([...direct.value,...dms].map(d=>[d.id,d])).values()];if(prefs.status==='fulfilled'&&prefVersion===preferenceVersion){preferences=prefs.value;preferencesLoaded=true;}if(result.some(r=>r.status==='rejected'))socialError='Some unread counts or preferences could not be loaded.';socialLoading=false;const desiredDm=dms.find(d=>d.id===desired);if(desiredDm&&historyVersion===expectedHistory)void openChannel(dmChannel(desiredDm));else scheduleAck();}
  function savedPreferences(value:NotificationPreferences){preferenceVersion++;preferences=value;preferencesLoaded=true;}
  function savedDevice(value:DeviceNotifications){const was=deviceNotifications.desktop;deviceNotifications=value;storeDeviceNotifications(value);if(value.desktop&&!was)void enablePush(pushApis());else if(!value.desktop&&was)void disablePush(pushApis());}
  function pushApis(){const apis=[api];for(const server of savedServers){if(server.origin===api.origin&&server.slug===api.slug)continue;const token=readSession(server.origin,server.slug);if(token)apis.push(new Api(server.origin,server.slug,token));}return apis;}
  function scheduleAck(){clearTimeout(ackTimer);if(reading())ackTimer=setTimeout(()=>void acknowledgeVisible(),350);}
  async function acknowledgeVisible(){if(!reading()||!messages.length)return;const latest=messages.at(-1)!.id,channel=activeId,target=api;const row=stateFor(channel);if(!newer(latest,row?.last_read_id)||ackInFlight.has(channel))return;ackInFlight.set(channel,latest);try{await target.request('/channels/'+encodeURIComponent(channel)+'/ack','POST',{message_id:latest});if(target===api){readStates=acknowledged(readStates,channel,latest);}}catch{/* A later visible scroll or refresh retries the acknowledgment. */}finally{if(target===api){ackInFlight.delete(channel);if(reading(channel)&&newer(messages.at(-1)?.id,latest))scheduleAck();}}}
  function refreshReadCounts(){clearTimeout(readRefreshTimer);const target=api;readRefreshTimer=setTimeout(async()=>{const before=readStates;try{const rows=await target.request<ReadState[]>('/read-states','GET',undefined,socialAbort?.signal);if(target===api)readStates=readStates===before?rows:mergeReads(readStates,rows);}catch{/* Visible channel activity or the next metadata refresh retries. */}},500);}
  function readVisibility(){if(document.visibilityState==='visible'){scheduleAck();if(snapshot&&!socialLoading)void loadSocial();}}
  function playNotification(){if(!deviceNotifications.sound||!deviceNotifications.volume)return;soundAudio??=new Audio('/sounds/notification.mp3');soundAudio.volume=deviceNotifications.volume/100;soundAudio.currentTime=0;void soundAudio.play().catch(()=>{if(!soundBlocked){soundBlocked=true;notify('Notification sound was blocked. Open Notifications and use Test sound to enable it.');}});}
  async function desktopNotice(title:string,body:string,tag:string,target:NotificationTarget){if(isDesktopApp){if(deviceNotifications.desktop&&(document.visibilityState==='hidden'||!document.hasFocus()))try{await desktopNotify(title,body,target,deviceNotifications.closed);}catch{/* The app refused (another page); nothing to show. */}return;}if(!deviceNotifications.desktop||document.visibilityState!=='hidden'||typeof Notification==='undefined'||Notification.permission!=='granted')return;try{const registration=await prepareNotifications();if(!deviceNotifications.desktop||document.visibilityState!=='hidden')return;if(registration){await registration.showNotification(title,{body,tag,silent:true,data:target});}else{const notice=new Notification(title,{body,tag,silent:true});notice.onclick=()=>{notice.close();window.focus();void activateNotification(target);};}}catch{notify('Browser notifications could not be displayed. Check notification settings.');}}
  async function activateNotification(value:unknown){const target=knownNotificationTarget(value,savedServers);if(!target)return;if(target.origin===origin&&target.slug===api.slug&&snapshot){modal=null;if(target.channel_id){let channel=snapshot.channels.find(c=>c.id===target.channel_id)??(dms.find(d=>d.id===target.channel_id)?dmChannel(dms.find(d=>d.id===target.channel_id)!):undefined);if(!channel){await loadSocial();channel=dms.find(d=>d.id===target.channel_id)?dmChannel(dms.find(d=>d.id===target.channel_id)!):undefined;}if(channel)await openChannel(channel);}}else{pendingNotificationTarget=target;await connectTarget(target);}}
  function notificationClick(event:MessageEvent){if(event.data?.type==='notification_click')void activateNotification(event.data.target);}

  function incoming(message:Message){if(!snapshot)return;const isDm=dms.some(d=>d.id===message.channel_id),ping=isDm||mentioned(message,snapshot.user.id,selfRoles());const fresh=newer(message.id,stateFor(message.channel_id)?.last_message_id);readStates=received(readStates,message,snapshot.user.id,isDm||mentioned(message,snapshot.user.id,selfRoles(),false));if(message.author.id===snapshot.user.id)readStates=acknowledged(readStates,message.channel_id,message.id);if(fresh&&!notifiedIds.has(message.id)&&!reading(message.channel_id)&&shouldNotify(message,snapshot.user.id,preferences,isDm,ping)){notifiedIds.add(message.id);if(notifiedIds.size>500)notifiedIds.delete(notifiedIds.values().next().value!);playNotification();void desktopNotice(message.author.display_name+' · '+snapshot.server.name,mentionParts(message.content,snapshot.members,snapshot.roles,snapshot.channels).map(part=>part.text).join('').slice(0,180)||'Sent an attachment',api.origin+':'+api.slug+':'+message.channel_id,{origin:api.origin,slug:api.slug,channel_id:message.channel_id})}scheduleAck();}
  async function openDirect(userId:string){if(!snapshot||userId===snapshot.user.id||busy)return;const target=api;busy=true;error='';try{const dm=await target.request<DirectMessage>('/dms','POST',{user_id:userId});if(target!==api)return;dms=[dm,...dms.filter(d=>d.id!==dm.id)];modal=null;await openChannel(dmChannel(dm));}catch(cause){if(target===api){error=messageError(cause);notify(error);}}finally{busy=false;}}
  function mentionMember(id:string){const member=snapshot?.members.find(m=>m.user.id===id);if(member){selectedMember=member;modal='member';}}
  function mentionChannel(id:string){const channel=snapshot?.channels.find(c=>c.id===id);if(channel){modal=null;void openChannel(channel);}}
  async function pickMention(value:string,cursor:number){draft=value;mentionCursor=0;await tick();composer?.focus();composer?.setSelectionRange(cursor,cursor);}
  async function jumpToMessage(message:Message){const channel=snapshot?.channels.find(c=>c.id===message.channel_id)??(dms.find(d=>d.id===message.channel_id)?dmChannel(dms.find(d=>d.id===message.channel_id)!):undefined);if(!channel){error='This conversation is no longer available.';return;}modal=null;await openChannel(channel,true,message.id);}
  async function pollSavedServers(){if(railPolling||!appAlive)return;railPolling=true;try{const saved=[...savedServers];for(let i=0;i<saved.length&&appAlive;i+=4)await Promise.allSettled(saved.slice(i,i+4).map(async server=>{if(server.origin===origin&&server.slug===api.slug)return;const token=readSession(server.origin,server.slug);if(!token)return;const target=new Api(server.origin,server.slug,token),deadline=new AbortController();const cancel=()=>deadline.abort();const timeout=setTimeout(cancel,10000);appAbort.signal.addEventListener('abort',cancel,{once:true});try{const [rows,prefs]=await Promise.all([target.request<ReadState[]>('/read-states','GET',undefined,deadline.signal),target.request<NotificationPreferences>('/me/preferences','GET',undefined,deadline.signal)]);if(!appAlive||readSession(server.origin,server.slug)!==token)return;const key=serverIdentity(server),previous=railReads.get(key);railBadges={...railBadges,[key]:{unread:rows.some(unread),mentions:rows.reduce((sum,row)=>sum+row.mention_count,0)}};if(previous?.token===token&&rows.some(row=>{const old=previous.rows.find(s=>s.channel_id===row.channel_id);const mode=prefs.channels[row.channel_id]??prefs.notify;return old&&newer(row.last_message_id,old.last_message_id)&&unread(row)&&(mode==='all'||mode==='mentions'&&row.mention_count>old.mention_count);})){playNotification();void desktopNotice(server.name,'New activity in a saved community',key,{origin:server.origin,slug:server.slug});}railReads.set(key,{token,rows});}catch{/* Offline hosts retain their last known badge until the next poll. */}finally{clearTimeout(timeout);appAbort.signal.removeEventListener('abort',cancel);}}));}finally{railPolling=false;}}

  function initials(name: string) { return name.trim().split(/\s+/).slice(0, 2).map(part => part[0]).join('').toUpperCase() || '?'; }
  function color(id: string) { return Number(id.slice(-2)) % 4 || 0; }
  function time(value: string) { return new Date(value).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }); }
  function date(value: string) { return new Date(value).toLocaleDateString([], { month: 'long', day: 'numeric', year: 'numeric' }); }
  function sameDay(a: string, b: string) { return new Date(a).toDateString() === new Date(b).toDateString(); }
  function messageError(cause: unknown) { return cause instanceof Error ? cause.message : 'Something went wrong. Please try again.'; }
  function notify(text: string) { toast = text; clearTimeout(toastTimer); toastTimer = setTimeout(() => toast = '', 5000); }
  function handleError(cause: unknown) {
    if (cause instanceof ApiError && cause.code === 'unauthorized') { expire(messageError(cause)); return; }
    const restriction = sendRestriction(cause, Date.now());
    if (restriction.cooldown) cooldownUntil = Math.max(cooldownUntil, restriction.cooldown);
    if (restriction.mutedUntil) mutedUntil = Math.max(mutedUntil, restriction.mutedUntil);
    chatError = messageError(cause) + (cause instanceof ApiError && cause.code === 'account_too_new' && cause.until ? ` Restricted until ${new Date(cause.until).toLocaleString()}.` : '');
  }
  async function refreshAiFlags() {
    const target = api, version = ++aiRefreshVersion;
    if (!snapshot || !can(snapshot.permissions, 2, snapshot.is_owner)) { aiFlags = []; aiMore = false; return; }
    aiLoading = true;
    const live = new Map<string, AiFlag>(); aiLiveUpdates = live;
    try { const page = await target.request<AiFlag[]>('/ai/flags?status=open&limit=100'); if (version === aiRefreshVersion && api === target && snapshot && can(snapshot.permissions, 2, snapshot.is_owner)) { aiFlags = mergeFlags(page, [...live.values()], 'open'); aiMore = page.length === 100; } }
    catch { /* The queue dialog provides a visible retry/error state. */ }
    finally { if (version === aiRefreshVersion) aiLoading = false; }
  }
  let aiLiveUpdates = new Map<string, AiFlag>();
  function updateAiFlag(flag: AiFlag) { aiLiveUpdates.set(flag.id, flag); latestAiFlag = flag; aiFlags = mergeFlags(aiFlags, [flag], 'open'); }
  function clearContext() {
    reacting = null; composeEmojiAnchor = null;
    hubHostServers=null;socialAbort?.abort();dms=[];readStates=[];preferences=defaultPreferences();preferencesLoaded=false;socialError='';socialLoading=false;viewingHistory=false;jumpId='';clearTimeout(ackTimer);clearTimeout(readRefreshTimer);ackInFlight.clear();notifiedIds.clear();
    aiRefreshVersion++; aiFlags = []; latestAiFlag = null; aiMore = false; aiLoading = false; aiLiveUpdates.clear();
    voice.leave(); voiceStates = []; voiceSpeaking = {}; channelViews.clear();
    contextVersion++; historyVersion++; historyAbort?.abort(); connection?.stop(); connection = undefined;
    snapshot = null; activeId = ''; messages = []; draft = ''; reply = null; editing = null; typing = {}; liveChanges.clear();
    ownReactionEvents.clear(); cooldownUntil = 0; mutedUntil = 0;
    attachments = []; uploading = false; recording = false; selectedMember = null; viewedAttachment = null; channelSettings = null;
    status = 'offline'; loading = false; chatError = ''; navOpen = false;
  }
  function restoreSavedSession() {
    if(snapshot || modal!=='auth' || api.token || !selected)return;
    const token=readSession(origin,api.slug);if(token){api.token=token;error='';startConnection();}
  }
  function sessionStorageChanged(event:StorageEvent){if(event.key==='chitchat.notifications'||event.key===null)deviceNotifications=loadDeviceNotifications();if(event.key===null||event.key===sessionKey(origin,api.slug))restoreSavedSession();}
  function expire(reason: string) { clearSession(origin, api.slug); clearContext(); modal = 'auth'; error = reason; }
  async function discover(address = origin) {
    busy = true; error = '';
    try {
      const nextOrigin = normalizeHost(address);
      const discovered = await new Api(nextOrigin).host();
      clearContext(); origin = nextOrigin; hostInput = origin; host = discovered;
      localStorage.setItem('chitchat.host', origin); sessionStorage.removeItem('chitchat.host'); modal = 'servers'; selected = null;
    } catch (cause) { error = messageError(cause); }
    finally { busy = false; }
  }
  function rememberJoined(server:ServerSummary){savedServers=rememberServer(savedServers,origin,server);if(!storeSavedServers(savedServers))notify('Your server list could not be saved in this browser.');}
  async function connectTarget(target:ServerTarget){
    if(busy)return;busy=true;error='';
    try{
      if(location.protocol==='https:'&&target.origin.startsWith('http:'))throw new Error('Use an HTTPS host address from this secure page.');
      const targetApi=new Api(target.origin,target.slug);
      const [nextHost,server]=await Promise.all([targetApi.host(),targetApi.publicServer()]);
      clearContext();origin=target.origin;hostInput=origin;host=nextHost;localStorage.setItem('chitchat.host',origin);sessionStorage.removeItem('chitchat.host');
      if(target.invite)await joinInvite(target.slug,target.invite);else await chooseServer(server);
    }catch(cause){error=messageError(cause);}finally{busy=false;}
  }
  async function removeSaved(server:SavedServer){
    const token=readSession(server.origin,server.slug),oldApi=new Api(server.origin,server.slug,token),active=origin===server.origin&&api.slug===server.slug;
    const cleanup=active?uploadTray?.discardPending():undefined;
    savedServers=savedServers.filter(item=>serverIdentity(item)!==serverIdentity(server));storeSavedServers(savedServers);clearSession(server.origin,server.slug);
    if(active){clearContext();api=new Api(origin);selected=null;modal='saved';}
    if(token)try{await cleanup;await oldApi.request('/auth/logout','POST');}catch{notify('Removed from your list. The host could not confirm sign-out.');}
  }
  async function chooseServer(server: ServerSummary, token?: string) {
    clearContext(); selected = server; error = ''; password = ''; serverPassword = ''; setupCode = ''; inviteCode = ''; invitePreview = null; showSetup = false;
    api = new Api(origin, server.slug, token ?? readSession(origin, server.slug));
    localStorage.setItem(`chitchat.last_server:${origin}`, server.slug);
    const search = location.pathname === `/s/${server.slug}` ? location.search : '';
    history.replaceState(null, '', `/s/${encodeURIComponent(server.slug)}${search}`);
    mode = 'login';
    modal = 'auth';
    if (api.token) startConnection();
  }
  function createdServer(result: CreatedServer) {
    let remember=false;try{remember=localStorage.getItem(sessionKey(origin,api.slug))===api.token;}catch{/* Use tab storage when persistence is unavailable. */}
    if(!saveSession(origin,result.server.slug,result.token,remember))notify('Browser storage is unavailable. Keep this tab open to stay signed in.');
    if(host)host={...host,servers:[...host.servers.filter(item=>item.slug!==result.server.slug),result.server]};
    void chooseServer(result.server,result.token);
  }
  async function findServer() {
    if (!/^[a-z0-9-]{2,32}$/.test(slugInput)) { error = 'Use a server slug with 2–32 lowercase letters, numbers, or hyphens.'; return; }
    busy = true; error = '';
    try { const server = await new Api(origin, slugInput).publicServer(); await chooseServer(server); }
    catch (cause) { error = messageError(cause); }
    finally { busy = false; }
  }
  async function joinInvite(slug: string, code: string) {
    busy = true; error = '';
    try {
      const preview = await new Api(origin, slug).request<InvitePreview>(`/invites/${encodeURIComponent(code)}`);
      await chooseServer(preview.server); invitePreview = preview; inviteCode = preview.code; mode = 'register';
      history.replaceState(null, '', `/s/${encodeURIComponent(slug)}/invite/${encodeURIComponent(code)}`);
    } catch (cause) { error = messageError(cause); modal = 'servers'; }
    finally { busy = false; }
  }
  async function authenticate(event: SubmitEvent) {
    event.preventDefault(); busy = true; error = '';
    const version = contextVersion, authApi = api;
    try {
      const result = await authApi.auth(mode, { username: username.toLowerCase().trim(), password, device_id: deviceId(), ...(mode === 'register' ? {
        display_name: displayName.trim() || username.trim(), ...(serverPassword ? { server_password: serverPassword } : {}), ...(inviteCode ? { invite_code: inviteCode } : {}), ...(setupCode ? { setup_code: setupCode } : {}),
      } : {}) });
      if (version !== contextVersion) return;
      api.token = result.token; const stored = saveSession(origin, api.slug, result.token, keepSignedIn); if (!stored) notify('Browser storage is unavailable. Your sign-in may last only for this tab.'); password = ''; setupCode = ''; serverPassword = ''; startConnection();
    } catch (cause) { if (version === contextVersion) error = messageError(cause); }
    finally { busy = false; }
  }
  function startConnection() {
    connection?.stop();
    const version = contextVersion;
    connection = new LiveConnection(api, event => { if (version === contextVersion) onEvent(event); }, next => { if (version === contextVersion) { status = next; if (next !== 'connected' && voice.view.channelId) voice.leave(false); } }, reason => { if (version === contextVersion) expire(reason); });
    connection.connect();
  }
  function onEvent(event: Envelope) {
    const data = event.data;
    if (event.type === 'ready') {
      snapshot = data as Snapshot; selected = snapshot.server; rememberJoined(snapshot.server);modal = null; error = '';
      void refreshAiFlags();if(!isDesktopApp&&deviceNotifications.desktop&&typeof Notification!=='undefined'&&Notification.permission==='granted')void enablePush([api]);
      dms=snapshot.dms??dms;preferences=snapshot.preferences??preferences;readStates=mergeReads(readStates,snapshot.read_states??[]);
      voiceStates = snapshot.voice_states ?? []; voiceSpeaking = {}; channelViews.clear();
      const requested=pendingNotificationTarget;const desiredId=requested?.origin===api.origin&&requested.slug===api.slug?requested.channel_id:new URLSearchParams(location.search).get('channel');if(requested?.origin===api.origin&&requested.slug===api.slug)pendingNotificationTarget=null;
      const id = snapshot.channels.find(channel => channel.id === (activeId || desiredId))?.id ?? snapshot.channels.find(channel => channel.type === 'text')?.id ?? snapshot.channels[0]?.id;
      if (id) void openChannel(snapshot.channels.find(channel => channel.id === id)!, false);
      void loadSocial(desiredId??undefined,historyVersion);void pollSavedServers();
      return;
    }
    if (!snapshot) return;
    if (event.type === 'emoji_create') { snapshot = { ...snapshot, emojis: upsert(snapshot.emojis ?? [], data as CustomEmoji) }; return; }
    if (event.type === 'emoji_delete') { snapshot = { ...snapshot, emojis: (snapshot.emojis ?? []).filter(emoji => emoji.id !== data.id) }; return; }
    if (event.type === 'pin_update') { updatePin(data.message_id, data.pinned, data.channel_id); return; }
    if (event.type === 'ai_notice_update') { snapshot = { ...snapshot, ai_moderation: data.ai_moderation }; return; }
    if (event.type === 'ai_flag_create' || event.type === 'ai_flag_update') { if (can(snapshot.permissions, 2, snapshot.is_owner)) updateAiFlag(data as AiFlag); return; }
    if (event.type === 'voice_sdp') { void voice.handleSdp(data); return; }
    if (event.type === 'voice_end') { voice.handleEnd(data.message || `Voice ended: ${data.reason}`); return; }
    if (event.type === 'voice_speaking') { voiceSpeaking = { ...voiceSpeaking, [data.user_id]: data.speaking }; return; }
    if (event.type === 'voice_state_update') { voiceStates = voiceStates.filter(item => item.user_id !== data.user_id); if (data.channel_id) voiceStates = [...voiceStates, data]; else voiceSpeaking = { ...voiceSpeaking, [data.user_id]: false }; voice.syncStates(voiceStates, snapshot.user.id); return; }
    if(event.type==='dm_create'){dms=[data as DirectMessage,...dms.filter(dm=>dm.id!==data.id)];return;}
    if(event.type==='read_state_update'){readStates=readUpdate(readStates,data);return;}
    if(event.type==='preferences_update'){savedPreferences(data as NotificationPreferences);return;}
    if(event.type==='message_update'||event.type==='message_delete')refreshReadCounts();
    if(event.type==='message_create'){dms=dms.map(dm=>dm.id===data.channel_id?{...dm,last_message_id:data.id}:dm);incoming(data as Message);}
    if (data.channel_id && data.channel_id !== activeId) {
      const cached = channelViews.get(data.channel_id);
      if (cached) {
        if (event.type === 'message_create' || event.type === 'message_update') cached.messages = mergeMessages(cached.messages, [mergeEventMessage(cached.messages.find(item => item.id === data.id), data)]);
        if (event.type === 'message_delete') cached.messages = cached.messages.filter(item => item.id !== data.id);
        if (event.type === 'reaction_add' || event.type === 'reaction_remove') cached.messages = cached.messages.map(item => item.id === data.message_id ? { ...item, reactions: updateReaction(item.reactions, data.emoji, data.user_id === snapshot!.user.id, event.type === 'reaction_add') } : item);
      }
    }
    if ((event.type === 'message_create' || event.type === 'message_update') && data.channel_id === activeId && (!viewingHistory||event.type==='message_update'&&messages.some(message=>message.id===data.id))) {
      const atBottom = !messageScroll || messageScroll.scrollHeight - messageScroll.scrollTop - messageScroll.clientHeight < 130;
      const message = mergeEventMessage(messages.find(message => message.id === data.id), data, ownReactionEvents.get(data.id));
      liveChanges.set(data.id, message); messages = mergeMessages(messages, [message]);
      if (atBottom || data.author.id === snapshot.user.id) void scrollBottom();
    } else if (event.type === 'message_delete' && data.channel_id === activeId) {
      liveChanges.set(data.id, null); messages = messages.filter(message => message.id !== data.id);
      if (reply?.id === data.id) reply = null;
      if (editing?.id === data.id) editing = null;
    } else if ((event.type === 'reaction_add' || event.type === 'reaction_remove') && data.channel_id === activeId) {
      if (data.user_id === snapshot.user.id) {
        const mine = ownReactionEvents.get(data.message_id) ?? new Map<string, boolean>();
        mine.set(data.emoji, event.type === 'reaction_add'); ownReactionEvents.set(data.message_id, mine);
      }
      messages = messages.map(message => message.id !== data.message_id ? message : { ...message, reactions: updateReaction(message.reactions, data.emoji, data.user_id === snapshot!.user.id, event.type === 'reaction_add') });
      const changed = messages.find(message => message.id === data.message_id); if (changed) liveChanges.set(changed.id, changed);
    } else if (event.type === 'typing_start' && data.channel_id === activeId) { typing = { ...typing, [data.user_id]: Date.now() + 8000 }; }
    else if (event.type === 'presence_update') { snapshot.members = snapshot.members.map(member => member.user.id === data.user_id ? { ...member, online: data.online, status: data.status ?? (data.online ? 'online' : 'offline') } : member); }
    else if (event.type === 'member_join' || event.type === 'member_update') { snapshot.members = [...snapshot.members.filter(member => member.user.id !== data.user.id), data as Member]; if (selectedMember?.user.id === data.user.id) selectedMember = data; profileSaved(data.user as User); }
    else if (event.type === 'member_leave') { snapshot.members = snapshot.members.filter(member => member.user.id !== data.user_id); }
    else if (event.type === 'channel_create' || event.type === 'channel_update') {
      snapshot.channels = upsert(snapshot.channels, data as Channel);
      if (data.id === activeId && !data.media_allowed) { attachments = []; uploading = false; }
      if (!activeId) void openChannel(data);
    } else if (event.type === 'channel_delete') {
      channelViews.delete(data.id); if (voice.view.channelId === data.id) voice.leave();
      snapshot.channels = snapshot.channels.filter(channel => channel.id !== data.id);
      if (activeId === data.id) { const next = snapshot.channels[0]; if (next) void openChannel(next); else { historyAbort?.abort(); historyVersion++; activeId = ''; messages = []; reply = null; draft = ''; } }
    } else if (event.type === 'category_create' || event.type === 'category_update') { snapshot.categories = upsert(snapshot.categories, data as Category); }
    else if (event.type === 'category_delete') { snapshot.categories = snapshot.categories.filter(category => category.id !== data.id); snapshot.channels = snapshot.channels.map(channel => channel.category_id === data.id ? { ...channel, category_id: null } : channel); }
    else if (event.type === 'role_create' || event.type === 'role_update') { snapshot.roles = upsert(snapshot.roles, data as Role); }
    else if (event.type === 'role_delete') { snapshot.roles = snapshot.roles.filter(role => role.id !== data.id); }
    else if (event.type === 'server_update') { snapshot.server = data; selected = data;rememberJoined(data); }
    else if (event.type === 'permissions_update') {
      const permissionRevision = ++permissionRefreshVersion;
      snapshot.permissions = data.permissions; snapshot.is_owner = data.is_owner;dms=dms.map(dm=>({...dm,my_permissions:data.permissions}));
      if (!can(data.permissions, 2, data.is_owner)) { aiRefreshVersion++; aiFlags = []; latestAiFlag = null; aiLiveUpdates.clear(); aiMore = false; aiLoading = false; if (modal === 'ai-review') modal = null; }
      else void refreshAiFlags();
      if (!can(data.permissions, 12, data.is_owner) || !can(data.permissions, 0, data.is_owner)) voice.leave();
      snapshot.channels = snapshot.channels.map(channel => ({ ...channel, my_permissions: data.channel_permissions?.[channel.id] ?? data.permissions }));
      if (!can(data.permissions, 0, data.is_owner)) { snapshot.channels=[];if(!dms.some(dm=>dm.id===activeId)){historyVersion++;historyAbort?.abort();activeId='';messages=[];attachments=[];reply=null;draft='';} }
      else {
        const version = contextVersion, permissions = data.permissions;
        void api.request<Channel[]>('/channels').then(refreshed => {
          if (version !== contextVersion || permissionRevision !== permissionRefreshVersion || !snapshot || snapshot.permissions !== permissions) return;
          snapshot = { ...snapshot, channels: refreshed };
          if (!activeId && refreshed.length) void openChannel(refreshed.find(channel => channel.type === 'text') ?? refreshed[0]);
        }).catch(cause => { if (version === contextVersion) handleError(cause); });
      }
      if (!can(data.channel_permissions?.[activeId] ?? data.permissions, 7, data.is_owner)) { void uploadTray?.discardPending(); attachments = []; uploading = false; }
      if (modal === 'admin' || modal === 'member' || modal === 'channel-settings') modal = null;
    }
    else if (event.type === 'error') { chatError = data.message; }
    snapshot = { ...snapshot };
  }
  async function scrollBottom() { await tick(); if (messageScroll) messageScroll.scrollTop = messageScroll.scrollHeight;scheduleAck(); }
  async function openChannel(channel: Channel, resetDraft = true, around?:string) {
    reacting = null; composeEmojiAnchor = null;
    if (resetDraft && activeId && active?.type !== 'voice' && !viewingHistory && messageScroll && !loading) {
      channelViews.delete(activeId); channelViews.set(activeId, { messages: [...messages], hasOlder, scrollTop: messageScroll.scrollTop, atBottom: messageScroll.scrollHeight - messageScroll.scrollTop - messageScroll.clientHeight < 80 });
      while (channelViews.size > 8) channelViews.delete(channelViews.keys().next().value!);
    }
    const cached = resetDraft&&!around ? channelViews.get(channel.id) : undefined;
    viewingHistory=!!around;jumpId=around??'';
    historyAbort?.abort(); historyAbort = new AbortController(); const version = ++historyVersion;
    activeId = channel.id; messages = cached ? [...cached.messages] : []; liveChanges = new Map(); ownReactionEvents = new Map(); hasOlder = cached?.hasOlder ?? false; chatError = ''; loading = channel.type !== 'voice'; olderBusy = false; typing = {}; navOpen = false;
    if (resetDraft) { reply = null; draft = ''; editing = null; attachments = []; uploading = false; }
    history.replaceState(null, '', `/s/${encodeURIComponent(api.slug)}?channel=${encodeURIComponent(channel.id)}`);
    if (channel.type === 'voice') return;
    try {
      const page = around?await api.request<Message[]>('/channels/'+encodeURIComponent(channel.id)+'/messages?limit=50&around='+encodeURIComponent(around),'GET',undefined,historyAbort.signal):await api.messages(channel.id, undefined, historyAbort.signal);
      if (version !== historyVersion) return;
      messages = mergeHistory(mergeMessages(cached?.messages ?? [], page), [], liveChanges, ownReactionEvents);
      hasOlder = cached ? cached.hasOlder || page.length === 50 : page.length === 50;
      loading = false;
      await tick(); if (version !== historyVersion || !messageScroll) return;
      if(around){document.getElementById('message-'+around)?.scrollIntoView({block:'center'});}else{messageScroll.scrollTop=cached&&!cached.atBottom?cached.scrollTop:messageScroll.scrollHeight;scheduleAck();}
    } catch (cause) { if (version === historyVersion && !(cause instanceof DOMException && cause.name === 'AbortError')) handleError(cause); }
    finally { if (version === historyVersion) loading = false; }
  }
  async function loadOlder() {
    if (!active || !messages.length || olderBusy) return;
    const version = historyVersion; olderBusy = true; const oldHeight = messageScroll.scrollHeight; const oldTop = messageScroll.scrollTop;
    try {
      const page = await api.messages(active.id, messages[0].id, historyAbort?.signal);
      if (version !== historyVersion) return;
      messages = mergeHistory(page, messages, liveChanges, ownReactionEvents); hasOlder = page.length === 50;
      await tick(); messageScroll.scrollTop = oldTop + messageScroll.scrollHeight - oldHeight;
    } catch (cause) { if (version === historyVersion && !(cause instanceof DOMException && cause.name === 'AbortError')) handleError(cause); }
    finally { if (version === historyVersion) olderBusy = false; }
  }
  async function sendMessage() {
    if (!active || sending || uploading || recording || !sendAllowed || status !== 'connected' || cooldownUntil > Date.now() || mutedUntil > Date.now()) return;
    if (draftLimits.error) { chatError = draftLimits.error; return; }
    const target = activeId, version = contextVersion, original = draft, tray=uploadTray, sentAttachments=attachments.map(attachment=>attachment.id); sending = true; chatError = '';
    tray?.holdForSend(sentAttachments);let sent=false;
    try {
      const message = await api.send(target, original.trim(), reply?.id, randomId(), sentAttachments);sent=true;tray?.finishSend(sentAttachments,true);
      if (version === contextVersion && target === activeId) { messages = mergeMessages(messages, [message]);readStates=acknowledged(readStates,target,message.id); if (draft === original){draft='';mentionCursor=0;} attachments = attachments.filter(attachment => !sentAttachments.includes(attachment.id)); reply = null;if(viewingHistory&&active)await openChannel(active,false); await scrollBottom(); composer?.focus(); }
    } catch (cause) { if (version === contextVersion) handleError(cause); }
    finally {if(!sent)tray?.finishSend(sentAttachments,false);sending = false; }
  }
  function composerKey(event: KeyboardEvent) { if(mentionMenu?.key(event))return; if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) { event.preventDefault(); void sendMessage(); } }
  async function saveEdit(event: SubmitEvent) {
    event.preventDefault(); if (!editing || actionBusy) return;
    const limits = contentLimits(editText, editing.attachments.length); if (limits.error) { chatError = limits.error; return; }
    const target = editing, version = contextVersion; actionBusy = true;
    try { const updated = await api.edit(target.channel_id, target.id, editText.trim()); if (version === contextVersion && target.channel_id === activeId) { messages = mergeMessages(messages, [updated]); editing = null; } }
    catch (cause) { if (version === contextVersion) handleError(cause); } finally { actionBusy = false; }
  }
  async function deleteMessage() {
    if (!deleting || actionBusy) return; actionBusy = true; error = ''; const target = deleting, version = contextVersion;
    try { await api.delete(target.channel_id, target.id); if (version === contextVersion) { if (target.channel_id === activeId) messages = messages.filter(message => message.id !== target.id); modal = null; deleting = null; } }
    catch (cause) { if (version === contextVersion) error = messageError(cause); } finally { actionBusy = false; }
  }
  async function react(message: Message, emoji: string) {
    const existing = message.reactions.find(reaction => reaction.emoji === emoji);
    if (!existing?.me && !can(active?.my_permissions, 8, snapshot?.is_owner)) return;
    const version = contextVersion;
    try { await api.react(message.channel_id, message.id, emoji, !!existing?.me); }
    catch (cause) { if (version === contextVersion) handleError(cause); }
  }
  async function createChannel(event: SubmitEvent) {
    event.preventDefault(); busy = true; error = ''; const version = contextVersion;
    try {
      const channel = await api.request<Channel>('/channels', 'POST', { name: channelName.trim(), type: channelType, topic: channelTopic.trim(), category_id: channelCategory || null, ...(channelType === 'voice' ? { user_limit: Number(channelLimit) } : {}) });
      if (version !== contextVersion || !snapshot) return;
      snapshot.channels = upsert(snapshot.channels, channel); snapshot = { ...snapshot }; modal = null; channelName = ''; channelTopic = ''; void openChannel(channel);
    } catch (cause) { if (version === contextVersion) error = messageError(cause); } finally { busy = false; }
  }
  async function logout() {
    const oldApi = api, cleanup=uploadTray?.discardPending(); clearSession(origin, api.slug); clearContext(); api = new Api(origin, oldApi.slug); error = ''; modal = 'auth';
    try { await cleanup;await oldApi.request('/auth/logout', 'POST'); } catch { notify('Signed out here. The host could not confirm session revocation.'); }
  }
  function closeModal() { if (busy || actionBusy) return; error = ''; modal = snapshot ? null : 'servers';scheduleAck(); }
  function confirmLink(href: string) { pendingLink = href; error = ''; modal = 'link'; }
  function childError(cause: unknown) { if (cause instanceof ApiError && ['unauthorized', 'timed_out', 'rate_limited', 'account_too_new'].includes(cause.code)) handleError(cause); }
  function scopedError(expected: Api) { return (cause: unknown) => { if (expected === api) childError(cause); }; }
  function memberClose(expected: Member, expectedApi: Api) { return () => { if (api === expectedApi && modal === 'member' && selectedMember?.user.id === expected.user.id) modal = null; }; }
  function joinVoice(channel: Channel) {
    if (!snapshot || status !== 'connected') return;
    if (!voiceAvailable) { notify('Voice needs a secure (https://) connection to this host.'); return; }
    if (!can(channel.my_permissions, 0, snapshot.is_owner) || !can(channel.my_permissions, 12, snapshot.is_owner)) { notify('You do not have permission to join this voice room.'); return; }
    if (voiceStates.filter(item => item.channel_id === channel.id && item.user_id !== snapshot?.user.id).length >= (channel.user_limit ?? 10)) { notify('This voice room is full.'); return; }
    void voice.join(channel.id, { speak: can(channel.my_permissions, 13, snapshot.is_owner), stream: can(channel.my_permissions, 14, snapshot.is_owner), suppressed: muted });
  }
  async function saveChannel(event: SubmitEvent) {
    event.preventDefault(); if (!channelSettings) return; busy = true; error = '';
    try { const { id, name, topic, category_id, media_allowed, user_limit } = channelSettings; const updated = await api.request<Channel>(`/channels/${id}`, 'PATCH', { name, topic, category_id, media_allowed, ...(channelSettings.type === 'voice' ? { user_limit: Number(user_limit ?? 10) } : {}), ...(channelSettings.parent_id ? { archived: !!channelSettings.archived } : {}) }); if (snapshot) snapshot = { ...snapshot, channels: upsert(snapshot.channels, updated) }; modal = null; }
    catch (cause) { error = messageError(cause); childError(cause); } finally { busy = false; }
  }
  async function deleteChannel() { if (!channelSettings) return; busy = true; error = ''; try { await api.request(`/channels/${channelSettings.id}`, 'DELETE'); modal = null; } catch (cause) { error = messageError(cause); childError(cause); } finally { busy = false; } }
  function dialog(node: HTMLElement) {
    const previous = document.activeElement as HTMLElement;
    queueMicrotask(() => (node.querySelector<HTMLElement>('input, button, textarea, a[href]') ?? node).focus());
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); closeModal(); }
      if (event.key !== 'Tab') return;
      const focusable = [...node.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), a[href], [tabindex="0"]')];
      const first = focusable[0], last = focusable.at(-1);
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    node.addEventListener('keydown', handler);
    return { destroy() { node.removeEventListener('keydown', handler); previous?.focus(); } };
  }
  onMount(() => {
    clock = setInterval(() => now = Date.now(), 1000);railTimer=setInterval(()=>void pollSavedServers(),60000);document.addEventListener('visibilitychange',readVisibility);navigator.serviceWorker?.addEventListener('message',notificationClick);stopDesktopClicks=onDesktopNotificationClick(target=>void activateNotification(target));
    void (async () => {
      const noticeParams=new URLSearchParams(location.search);const noticeSlug=/^\/s\/([a-z0-9-]{2,32})/.exec(location.pathname)?.[1];const noticeTarget=knownNotificationTarget({origin:noticeParams.get('notify_host'),slug:noticeSlug,...(noticeParams.get('channel')?{channel_id:noticeParams.get('channel')}:{})},savedServers);if(noticeTarget){pendingNotificationTarget=noticeTarget;await connectTarget(noticeTarget);return;}
      // Alternate hosts persist only after an explicit choice on this browser.
      const savedHost=localStorage.getItem('chitchat.host') ?? sessionStorage.getItem('chitchat.host');
      // The desktop app is not a host itself: until you pick one, it asks where to go.
      if(isDesktopApp&&!savedHost){modal=savedServers.length?'saved':'host';hostInput='';return;}
      await discover(savedHost ?? origin);
      const inviteRoute = /^\/s\/([a-z0-9-]{2,32})\/invite\/([^/]+)\/?$/.exec(location.pathname);
      if (inviteRoute && host) { await joinInvite(inviteRoute[1], decodeURIComponent(inviteRoute[2])); return; }
      const routeSlug = /^\/s\/([a-z0-9-]{2,32})(?:\/|$)/.exec(location.pathname)?.[1];
      const slug = routeSlug ?? localStorage.getItem(`chitchat.last_server:${origin}`);
      if (host && slug) {
        const listed = host.servers.find(server => server.slug === slug);
        if (listed) await chooseServer(listed);
        else if (routeSlug) { slugInput = routeSlug; await findServer(); }
      }
    })();
  });
  onDestroy(() => { appAlive=false;appAbort.abort();socialAbort?.abort();clearTimeout(ackTimer);clearTimeout(readRefreshTimer);clearInterval(railTimer);soundAudio?.pause();document.removeEventListener('visibilitychange',readVisibility);navigator.serviceWorker?.removeEventListener('message',notificationClick);stopDesktopClicks();voiceWakeLock.dispose(); voice.leave(); connection?.stop(); historyAbort?.abort(); clearInterval(clock); clearTimeout(toastTimer); });
</script>

<svelte:window onfocus={()=>{restoreSavedSession();scheduleAck();}} onpageshow={restoreSavedSession} onstorage={sessionStorageChanged}/>
<svelte:head><title>{active ? `${active.type==='dm'?'':'#'}${active.name} · ` : ''}{snapshot?.server.name ? `${snapshot.server.name} · ` : ''}ChitChat</title></svelte:head>

<div class:nav-open={navOpen} class="app-shell" inert={modal !== null}>
  <button class="mobile-shade" aria-label="Close channel navigation" onclick={() => navOpen = false}></button>
  <nav class="server-rail" aria-label="Servers">
    <div class="brand-mark" title="ChitChat"><Icon name="chat" size={23}/></div>
    <div class="rail-divider"></div>
    {#each savedServers as server (serverIdentity(server))}
      {@const badge=origin===server.origin&&api.slug===server.slug?currentBadge:railBadges[serverIdentity(server)]}
      <button class="server-button" class:has-unread={badge?.unread} class:active={origin===server.origin&&selected?.slug===server.slug} title={`${server.name} · ${server.origin}`} aria-label={`Open ${server.name} (${new URL(server.origin).host})`} aria-describedby={`rail-status-${encodeURIComponent(serverIdentity(server))}`} aria-current={origin===server.origin&&selected?.slug===server.slug?'true':undefined} onclick={()=>connectTarget(server)}><span class="visually-hidden" id={`rail-status-${encodeURIComponent(serverIdentity(server))}`}>{badge?.mentions?`${badge.mentions} mentions`:badge?.unread?'Unread messages':'No unread messages'}</span>{initials(server.name)}{#if badge?.mentions}<span class="unread-count rail-count" aria-label={`${badge.mentions} mentions`}>{badge.mentions>99?'99+':badge.mentions}</span>{:else if badge?.unread}<span class="rail-unread" aria-label="Unread messages"></span>{/if}</button>
    {/each}
    <button class="server-button rail-add" title="Add or manage servers" aria-label="Add or manage servers" onclick={openServerHub}><Icon name="plus"/></button>
    <div class="rail-bottom"><Icon name="shield" size={18}/><span class="eyebrow" style="font-size:8px;letter-spacing:1px">Yours.</span></div>
  </nav>

  <aside class="channel-sidebar" aria-label="Channel navigation">
    <div class="server-heading"><h1>{snapshot?.server.name ?? 'ChitChat'}</h1><p><Icon name="globe" size={11}/><button class="host-switch" title={origin} aria-label={`Change host (current: ${origin})`} onclick={() => { error = ''; hostInput = origin; modal = 'host'; }}>{new URL(origin).host}</button></p></div>
    <div class="sidebar-scroll">
      {#if snapshot}<div class="social-tools"><button class="secondary" onclick={()=>modal='search'}>Search messages</button><button class="secondary" onclick={()=>modal='notifications'}>Notifications</button></div>{/if}
      {#if socialError}<button class="small danger" disabled={socialLoading} onclick={()=>loadSocial()}>{socialError} Retry</button>{/if}

      {#each [null, ...categories] as category}
        {@const group = channels.filter(channel => category ? channel.category_id === category.id : !channel.category_id || !categories.some(item => item.id === channel.category_id))}
        {#if group.length || (!category && !channels.length)}
          <div class="sidebar-label"><span class="eyebrow">{category?.name ?? 'Channels'}</span>{#if manageChannels}<button class="icon-button" title="Create a channel" aria-label="Create a channel" onclick={() => { channelCategory = category?.id ?? ''; error = ''; modal = 'channel'; }}><Icon name="plus" size={15}/></button>{/if}</div>
          {#each group as channel}
            <div class="channel-line"><button class="channel-item" class:selected={activeId === channel.id} class:has-unread={unread(stateFor(channel.id,readStates))} aria-current={activeId === channel.id ? 'page' : undefined} onclick={() => openChannel(channel)}><Icon name={channel.type === 'voice' ? 'people' : 'hash'} size={18}/><span>{channel.name}</span>{#if stateFor(channel.id,readStates)?.mention_count}<span class="unread-count" aria-label={`${stateFor(channel.id,readStates)!.mention_count} mentions`}>{stateFor(channel.id,readStates)!.mention_count>99?'99+':stateFor(channel.id,readStates)!.mention_count}</span>{/if}{#if channel.type === 'voice'}<span class="small muted">{voiceStates.filter(item => item.channel_id === channel.id).length}/{channel.user_limit ?? 10}</span>{/if}</button>{#if channel.type === 'voice'}{#if $voiceStatus.channelId === channel.id}<button class="voice-channel-action danger" aria-label={`Leave voice ${channel.name}`} onclick={() => voice.leave()}>Leave</button>{:else if voiceAvailable && can(channel.my_permissions, 12, snapshot?.is_owner)}<button class="voice-channel-action" aria-label={`Join voice ${channel.name}`} disabled={status !== 'connected'} onclick={() => joinVoice(channel)}>Join</button>{/if}{/if}</div>
            {#if channel.type === 'voice'}{#each voiceStates.filter(item => item.channel_id === channel.id) as occupant}<div class="voice-subscriber" class:is-speaking={occupant.user_id === snapshot?.user.id ? $voiceStatus.speaking : voiceSpeaking[occupant.user_id]}><span class="status-dot"></span>{snapshot?.members.find(member => member.user.id === occupant.user_id)?.user.display_name ?? 'Member'}{occupant.self_mute ? ' · muted' : ''}{occupant.streaming ? ' · sharing' : ''}</div>{/each}{/if}
            {#if channel.type === 'text'}{#each (snapshot?.channels ?? []).filter(thread => thread.parent_id === channel.id && !thread.archived) as thread}<button class="channel-item thread-nav" class:selected={activeId === thread.id} class:has-unread={unread(stateFor(thread.id,readStates))} aria-label={`Open thread ${thread.name}`} onclick={() => openChannel(thread)}><Icon name="chat" size={14}/><span>{thread.name}</span>{#if stateFor(thread.id,readStates)?.mention_count}<span class="unread-count">{stateFor(thread.id,readStates)!.mention_count}</span>{/if}</button>{/each}{/if}
          {/each}
        {/if}
      {/each}
      {#if snapshot}<div class="sidebar-label"><span class="eyebrow">Direct messages</span><button class="icon-button" aria-label="New direct message" disabled={!can(snapshot.permissions,16,snapshot.is_owner)} onclick={()=>{dmQuery='';error='';modal='dm-picker';}}><Icon name="plus" size={15}/></button></div>
      {#each sortedDms as dm(dm.id)}<button class="channel-item dm-item" class:selected={activeId===dm.id} class:has-unread={unread(stateFor(dm.id,readStates))} aria-label={`Open direct message with ${dm.user.display_name}`} aria-current={activeId===dm.id?'page':undefined} aria-describedby={`dm-status-${dm.id}`} onclick={()=>openChannel(dmChannel(dm))}><span class="visually-hidden" id={`dm-status-${dm.id}`}>{stateFor(dm.id,readStates)?.mention_count?`${stateFor(dm.id,readStates)!.mention_count} unread messages`:unread(stateFor(dm.id,readStates))?'Unread messages':'No unread messages'}</span><Icon name="chat" size={17}/><span>{dm.user.display_name}</span>{#if stateFor(dm.id,readStates)?.mention_count}<span class="unread-count">{stateFor(dm.id,readStates)!.mention_count>99?'99+':stateFor(dm.id,readStates)!.mention_count}</span>{/if}</button>{/each}
      {#if !dms.length}<p class="small muted dm-empty">No direct messages yet.</p>{/if}{/if}
      <div class="sidebar-note"><Icon name="lock" size={13}/><br/>Hosted by your community.<br/>Accounts stay on this server.</div>
    </div>
    <div class="user-panel">{#if snapshot}<button class="icon-button" aria-label="Edit your profile" title="Your profile" onclick={() => modal = 'profile'}><Avatar user={snapshot.user} {origin} status={selfMember ? memberPresence(selfMember) : 'online'}/></button>{:else}<div class="avatar">?</div>{/if}<div class="identity"><strong>{snapshot?.user.display_name ?? 'Not signed in'}</strong><span>{snapshot?.user.status_text || (snapshot?.user.status === 'away' ? 'Away' : snapshot?.is_owner ? 'Server owner' : snapshot ? `@${snapshot.user.username}` : 'Welcome to ChitChat')}</span></div><button class="icon-button" title="Appearance" aria-label="Appearance" onclick={()=>modal='appearance'}><Icon name="palette" size={17}/></button>{#if snapshot}<button class="icon-button" title="Sign out of this server" aria-label="Sign out of this server" onclick={logout}><Icon name="logout" size={17}/></button>{/if}</div>
  </aside>

  <main class="main">
    <header class="channel-header">
      <button class="icon-button mobile-only" aria-label="Open channel navigation" onclick={() => navOpen = true}><Icon name="menu"/></button>
      <div class="header-hash"><Icon name={active?.type==='dm'?'chat':'hash'} size={25}/></div>
      <div><h2>{active?.name ?? 'Welcome home'}</h2><p>{active?.topic || (active ? 'A little space to keep the conversation going.' : 'Your people. Your space. Your server.')}</p></div>
      <div class="header-actions">{#if snapshot}{#if active && active.type !== 'voice'}<button class="icon-button" aria-label="Pinned messages" title="Pinned messages" onclick={() => modal = 'pins'}><Icon name="pin" size={18}/></button>{#if active.type === 'text'}<button class="icon-button" aria-label="View threads" title="Threads" onclick={() => modal = 'threads'}><Icon name="chat" size={18}/></button>{#if can(snapshot?.permissions, 4, snapshot?.is_owner)}<button class="icon-button" aria-label="Channel posting rules" title="Posting rules" onclick={() => { channelSettings = { ...active! }; modal = 'channel-permissions'; }}><Icon name="shield" size={18}/></button>{/if}{/if}{/if}
      <button class="icon-button" title="Search messages" aria-label="Search messages" onclick={()=>modal='search'}><Icon name="search" size={19}/></button><button class="icon-button" title="Notifications" aria-label="Notification settings" onclick={()=>modal='notifications'}><Icon name="bell" size={19}/></button>{#if dmNoticeCollapsed && active?.type==='dm'}<button class="ai-notice-chip" aria-label="Show encryption notice" aria-expanded="false" title="Not end-to-end encrypted: the host's admin can read DMs." onclick={() => setDmNoticeCollapsed(false)}>Not E2E</button>{/if}{#if aiNoticeCollapsed && coversChannel(snapshot.ai_moderation, active)}<button class="ai-notice-chip" aria-label="Show AI moderation notice" aria-expanded="false" title={`AI moderation: ${snapshot.ai_moderation!.providers.join(', ')}${snapshot.ai_moderation!.cloud ? ' · Messages sent to an outside AI service' : ''}`} onclick={() => setAiNoticeCollapsed(false)}>AI{snapshot.ai_moderation!.cloud ? ' · Cloud' : ''}</button>{/if}<span class="live-badge"><span class="status-dot" class:offline={status !== 'connected'}></span>{status === 'connected' ? 'Connected' : status === 'reconnecting' ? 'Reconnecting' : 'Connecting'}</span>{#if active && active.type!=='dm' && manageChannels}<button class="icon-button" title="Channel settings" aria-label="Channel settings" onclick={() => { channelSettings = { ...active! }; confirmChannelDelete = false; error = ''; modal = 'channel-settings'; }}><Icon name="edit" size={18}/></button>{/if}{#if can(snapshot.permissions, 2, snapshot.is_owner)}<button class="secondary ai-review-button" aria-label={`AI review queue, ${aiFlags.length}${aiMore ? '+' : ''} open flags`} onclick={() => modal = 'ai-review'}>AI review <span class="ai-count">{aiLoading ? '…' : `${aiFlags.length}${aiMore ? '+' : ''}`}</span></button>{/if}{#if settingsAllowed}<button class="icon-button" title="Server settings" aria-label="Server settings" onclick={() => modal = 'admin'}><Icon name="shield" size={19}/></button>{/if}<button class="icon-button" aria-label="Toggle members" aria-pressed={showMembers} title="Members" onclick={() => showMembers = !showMembers}><Icon name="people"/></button>{/if}</div>
    </header>
    {#if chatError}<div class="banner error" role="alert"><span>{chatError}</span><button class="icon-button" aria-label="Dismiss error" onclick={() => chatError = ''}><Icon name="close" size={16}/></button></div>{/if}
    {#if snapshot && status !== 'connected'}<div class="banner notice" role="status">Connection interrupted. Reconnecting to your server…</div>{/if}
    {#if snapshot && muted}<div class="banner notice" role="status">Muted until {new Date(mutedUntil).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}. You can still read messages. Your draft is saved here.</div>{/if}
    <div class="chat-body">
      <section class="conversation" aria-label="Conversation">
        {#if snapshot}<VoicePanel {origin} panelScope={`${origin}:${api.slug}`} client={voice} {snapshot} selected={active?.type === 'voice' ? active : undefined} states={voiceStates} speaking={voiceSpeaking} connected={status === 'connected'} onJoin={joinVoice}/>{/if}
        {#if active && active.type !== 'voice'}
          {#if active.parent_id}<div class="banner notice"><span>Thread{#if activeParent} in #{activeParent.name}{/if}{active.archived ? ' · Archived' : ''}</span>{#if activeParent}<div class="button-row">{#if active.root_message_id}<button class="secondary" onclick={jumpThreadRoot}>View starter</button>{/if}<button class="secondary" onclick={() => openChannel(activeParent!)}>Back to channel</button></div>{/if}</div>{/if}
          {#if active.type==='dm'&&!dmNoticeCollapsed}<div class="banner notice dm-privacy ai-channel-notice" role="status"><span>Not end-to-end encrypted: the host's admin can read DMs.</span><button class="icon-button" aria-label="Collapse encryption notice" title="Collapse notice" onclick={() => setDmNoticeCollapsed(true)}><Icon name="close" size={16}/></button></div>{/if}
          {#if viewingHistory}<div class="banner notice"><span>Viewing earlier messages. New messages stay unread.</span><button class="secondary" onclick={()=>openChannel(active!,false)}>Back to latest</button></div>{/if}
          {#if !aiNoticeCollapsed && coversChannel(snapshot?.ai_moderation, active)}<div class="banner notice ai-channel-notice" role="status"><span>Messages here are checked by an AI moderator: {snapshot!.ai_moderation!.providers.join(', ')}. {#if snapshot!.ai_moderation!.cloud}<strong>Messages are sent to an outside AI service.</strong>{/if} Checks run in the background.</span><button class="icon-button" aria-label="Collapse AI moderation notice" title="Collapse notice" onclick={() => setAiNoticeCollapsed(true)}><Icon name="close" size={16}/></button></div>{/if}
          <div class="message-scroll" bind:this={messageScroll} onscroll={scheduleAck}>
            {#if hasOlder}<button class="load-older" disabled={olderBusy} onclick={loadOlder}>{olderBusy ? 'Loading history…' : 'Load earlier messages'}</button>{:else if !loading}<div class="channel-welcome"><div class="welcome-icon"><Icon name="hash" size={31}/></div><span class="eyebrow">This is where it begins</span><h2 style="margin-top:9px">{active.type==='dm'?'Conversation with ':'Welcome to #'}{active.name}</h2><p>{active.topic || 'Say hello, share a thought, or pick up where you left off. This is your space to make a little more yours.'}</p></div>{/if}
            {#if loading}<div class="loading" role="status">Loading the conversation…</div>{/if}
            {#each messages as message, index (message.id)}
              {#if index === 0 || !sameDay(messages[index - 1].created_at, message.created_at)}<div class="date-divider">{date(message.created_at)}</div>{/if}
              <article id={`message-${message.id}`} class="message" class:mentioned={snapshot&&message.author.id!==snapshot.user.id&&mentioned(message,snapshot.user.id,selfRoles())} class:search-jump={message.id===jumpId} aria-label={`Message from ${message.author.display_name}`}>
                <Avatar user={snapshot?.members.find(member => member.user.id === message.author.id)?.user ?? message.author} {origin}/>
                <div class="message-content">
                  {#if message.reply_to}<div class="reply-reference"><Icon name="reply" size={11}/> {messages.find(item => item.id === message.reply_to)?.content.slice(0, 100) ?? 'Reply to an earlier message'}</div>{/if}
                  <div class="message-meta"><strong>{message.author.display_name}</strong>{#if message.pinned}<span class="small muted" title="Pinned message"><Icon name="pin" size={12}/></span>{/if}<time datetime={message.created_at} title={new Date(message.created_at).toLocaleString()}>{time(message.created_at)}</time></div>
                  {#if editing?.id === message.id}
                    <form class="edit-form" onsubmit={saveEdit}><textarea aria-label="Edit message" bind:value={editText} required={!editing.attachments.length}></textarea><div class="edit-buttons"><button type="submit" class="primary" disabled={actionBusy || !!contentLimits(editText, editing.attachments.length).error}>Save changes</button><button type="button" class="secondary" onclick={() => editing = null}>Cancel</button></div></form>
                  {:else}<p class="message-text"><MessageText content={message.content} onLink={confirmLink} members={snapshot?.members??[]} roles={snapshot?.roles??[]} channels={snapshot?.channels??[]} selfId={snapshot?.user.id??''} onMember={mentionMember} onChannel={mentionChannel} emojis={snapshot?.emojis ?? []} {origin}/>{#if message.edited_at}<span class="edited">(edited)</span>{/if}</p>{/if}
                  {#if message.thread_id}<button class="small muted" onclick={() => startThread(message)}>Open thread →</button>{/if}
                  {#if message.attachments?.length}<MessageMedia attachments={message.attachments} {origin} onView={attachment => { viewedAttachment = attachment; modal = 'media'; }}/>{/if}
                  {#if message.reactions.length}<div class="reactions">{#each message.reactions as reaction}<button class="reaction" class:mine={reaction.me} aria-label={`${reaction.me ? 'Remove' : 'Add'} ${reactionName(reaction.emoji, snapshot?.emojis ?? [])} reaction, ${reaction.count}`} aria-pressed={reaction.me} disabled={!reaction.me && !can(active.my_permissions, 8, snapshot?.is_owner)} onclick={() => react(message, reaction.emoji)}><EmojiGlyph value={reaction.emoji} emojis={snapshot?.emojis ?? []} {origin}/> {reaction.count}</button>{/each}{#if can(active.my_permissions, 8, snapshot?.is_owner) && message.reactions.length < 20}<button class="reaction add-reaction" aria-label="Add a reaction" title="Add a reaction" onclick={event => reacting = { message, anchor: event.currentTarget }}><Icon name="plus" size={12}/></button>{/if}</div>{/if}
                </div>
                <div class="message-actions">
                  {#if active.type === 'dm' || can(snapshot?.permissions, 2, snapshot?.is_owner)}<button class="icon-button" aria-label={message.pinned ? 'Unpin message' : 'Pin message'} title={message.pinned ? 'Unpin' : 'Pin'} disabled={actionBusy || muted} onclick={() => togglePin(message)}><Icon name="pin" size={14}/></button>{/if}
                  {#if active.type === 'text' && !active.parent_id && (sendAllowed || message.thread_id)}<button class="icon-button" aria-label={message.thread_id ? 'Open thread' : 'Start thread'} title={message.thread_id ? 'Open thread' : 'Start thread'} disabled={actionBusy} onclick={() => startThread(message)}><Icon name="chat" size={14}/></button>{/if}
                  {#if sendAllowed}<button class="icon-button" aria-label="Reply to message" title="Reply" onclick={() => { reply = message; composer?.focus(); }}><Icon name="reply" size={15}/></button>{/if}
                  {#if can(active.my_permissions, 8, snapshot?.is_owner)}<button class="icon-button" aria-label="Add a reaction" title="Add a reaction" aria-haspopup="dialog" aria-expanded={reacting?.message.id === message.id} onclick={event => { const anchor = event.currentTarget; reacting = reacting?.anchor === anchor ? null : { message, anchor }; }}><Icon name="smile" size={15}/></button>{/if}
                  {#if message.author.id === snapshot?.user.id && sendAllowed}<button class="icon-button" aria-label="Edit message" title="Edit" onclick={() => { editing = message; editText = message.content; }}><Icon name="edit" size={14}/></button>{/if}
                  {#if message.author.id === snapshot?.user.id || active.type!=='dm'&&can(active.my_permissions, 2, snapshot?.is_owner)}<button class="icon-button danger" aria-label="Delete message" title="Delete" onclick={() => { deleting = message; error = ''; modal = 'delete'; }}><Icon name="trash" size={14}/></button>{/if}
                </div>
              </article>
            {/each}
            {#if reacting && reacting.message.channel_id === activeId}{@const target = reacting.message}<EmojiPicker extra={customEmojiGroups} anchor={reacting.anchor} onPick={emoji => react(messages.find(item => item.id === target.id) ?? target, emoji)} onClose={() => reacting = null}/>{/if}
          </div>
          <div class="composer-area" use:fileInput={{ enabled: attachAllowed && !uploading && !recording && !muted && status === 'connected', add: files => uploadTray?.addFiles(files) }}>
            <form class="composer" onsubmit={event => { event.preventDefault(); void sendMessage(); }}>
              <PendingAttachments {origin} bind:attachments disabled={sending} inComposer={true}/>
              {#if reply}<div class="composer-context"><Icon name="reply" size={14}/><span>Replying to <strong>{reply.author.display_name}</strong></span><button type="button" class="icon-button" aria-label="Cancel reply" onclick={() => reply = null}><Icon name="close" size={14}/></button></div>{/if}
              {#if snapshot&&sendAllowed}<MentionMenu bind:this={mentionMenu} {snapshot} value={draft} cursor={mentionCursor} onPick={pickMention}/>{/if}
              <div class="composer-row"><textarea bind:this={composer} bind:value={draft} aria-label={`Message ${active.type==='dm'?'':'#'}${active.name}`} placeholder={sendAllowed ? `Message ${active.type==='dm'?'':'#'}${active.name}` : 'You can read, but cannot send messages in this channel.'} disabled={!sendAllowed} onkeydown={composerKey} oninput={event=>{mentionCursor=event.currentTarget.selectionStart;connection?.typing(activeId);}} onclick={event=>mentionCursor=event.currentTarget.selectionStart} onkeyup={event=>{if(!['Enter','Tab','Escape','ArrowUp','ArrowDown'].includes(event.key))mentionCursor=event.currentTarget.selectionStart;}}></textarea>{#if attachAllowed && (snapshot?.server.voice_messages ?? true)}{#key `${api.origin}:${api.slug}:${activeId}`}<VoiceRecorder enabled={attachAllowed && !muted && status==='connected'} canStart={!sending && !uploading && attachments.length<10 && cooldownSeconds===0} bind:busy={recording} onAttach={async file=>{const added=await uploadTray?.addFiles([file],true);if(!added)throw new Error('The recording was not attached. Check the upload error below and try again.');}}/>{/key}{/if}<button class="send-button" type="submit" title={muted ? 'Temporarily muted' : cooldownSeconds ? `Wait ${cooldownSeconds} seconds` : 'Send message'} aria-label="Send message" disabled={!sendAllowed || !!draftLimits.error || sending || uploading || recording || status !== 'connected' || cooldownSeconds > 0 || muted}><Icon name="send" size={17}/></button></div>
            </form>
            {#if attachAllowed}{#key `${api.origin}:${api.slug}:${activeId}`}<UploadTray bind:this={uploadTray} {api} channel={activeId} showTray={false} blocked={recording} enabled={attachAllowed && !muted && !sending && status === 'connected'} bind:attachments bind:uploading onError={handleError}/>{/key}{:else}<p class="small muted upload-hint">Attachments are disabled here or require the Attach files permission.</p>{/if}
            <div class="button-row"><button class="secondary small" disabled={!sendAllowed || muted || status !== 'connected'} onclick={event => composeEmojiAnchor = event.currentTarget}><Icon name="smile" size={15}/>Emoji</button>{#if can(snapshot?.permissions, 10, snapshot?.is_owner)}<button class="secondary small" onclick={() => modal = 'server-emojis'}>Manage server emoji</button>{/if}</div>
            {#if composeEmojiAnchor}<EmojiPicker anchor={composeEmojiAnchor} extra={customEmojiGroups} label="Choose an emoji" onPick={value => { void composeEmoji(value); }} onClose={() => composeEmojiAnchor = null}/>{/if}
            <label class="screen-awake-option"><input type="checkbox" checked={keepScreenOn} onchange={event=>setKeepScreenOn(event.currentTarget.checked)}/>Keep screen on while ChitChat is open</label>
            <div class="composer-footer"><span>{cooldownSeconds ? `Wait ${cooldownSeconds}s before sending again. Draft kept.` : 'Enter to send · Shift + Enter for a new line'}</span><span class:danger={draftLimits.characters > 4000 || draftLimits.lines > 60}>{draft.length ? `${draftLimits.characters.toLocaleString()} / 4,000${draftLimits.lines > 50 ? ` · ${draftLimits.lines}/60 lines` : ''}` : 'Make yourself at home.'}</span></div>
            {#if draft.trim() && draftLimits.error}<p class="danger small" role="status">{draftLimits.error}</p>{/if}
            <div class="typing-line" role="status">{typingNames.length ? `${typingNames.slice(0, 3).join(', ')} ${typingNames.length === 1 ? 'is' : 'are'} typing…` : ''}</div>
          </div>
        {:else if active?.type !== 'voice'}<div class="empty-state"><div class="welcome-icon" style="margin:0 auto 20px"><Icon name="chat" size={29}/></div><h2>{snapshot ? 'A fresh start.' : 'A place for your people.'}</h2><p>{snapshot ? 'This server has no channels yet.' : 'Join a community hosted by someone you trust. Your conversations have a home of their own.'}</p>{#if manageChannels}<button class="primary" onclick={() => { error = ''; modal = 'channel'; }}><Icon name="plus" size={17}/>Create a channel</button>{:else if !snapshot}<button class="primary" onclick={() => { modal = 'servers'; error = ''; }}>Find your server<Icon name="arrow" size={17}/></button>{/if}</div>{/if}
      </section>
      {#if snapshot && showMembers}<aside class="members-panel mobile-visible" aria-label="Server members"><p class="eyebrow">The people here · {snapshot.members.length}</p><p class="small muted">{onlineMembers.length} online</p>{#each sortedMembers as member}<button class="member member-button" aria-label={`View member ${member.user.display_name}`} onclick={() => { selectedMember = member; modal = 'member'; }}><Avatar user={member.user} {origin} status={memberPresence(member)}/><span><strong>{member.user.display_name}</strong>{#if member.user.status_text}<span class="member-status">{member.user.status_text}</span>{/if}<span class="member-status">{member.is_owner ? 'Server owner' : member.timed_out_until && Date.parse(member.timed_out_until) > now ? 'Timed out' : memberPresence(member) === 'away' ? 'Away' : member.online ? 'Online' : 'Offline'}</span></span></button>{/each}</aside>{/if}
    </div>
  </main>
</div>

{#if modal}
  {#key modal}
  <div class="modal-backdrop">
    <div class="modal" class:wide-modal={modal === 'admin' || modal === 'ai-review'} class:media-modal={modal === 'media'} role="dialog" aria-modal="true" aria-labelledby="dialog-title" tabindex="-1" use:dialog>
      {#if snapshot||modal==='appearance'}<button class="icon-button close-modal" aria-label="Close dialog" onclick={closeModal}><Icon name="close"/></button>{/if}
      {#if modal === 'saved' || modal === 'servers' || modal === 'browse' || modal === 'host-create'}
        <nav class="server-hub-nav" aria-label="Server menu">
          <button class:active={modal==='saved'} aria-current={modal==='saved'?'page':undefined} onclick={()=>{error='';modal='saved';}}>Your servers / Add link</button>
          <button class:active={modal==='servers'} aria-current={modal==='servers'?'page':undefined} onclick={()=>{error='';modal='servers';}}>Join this host</button>
          <button class:active={modal==='browse'} aria-current={modal==='browse'?'page':undefined} onclick={()=>{error='';modal='browse';}}>Search other hosts</button>
          {#if hubHostServers && snapshot}<button class:active={modal==='host-create'} aria-current={modal==='host-create'?'page':undefined} onclick={()=>{error='';modal='host-create';}}>Create server</button>{/if}
        </nav>
      {/if}
      {#if modal === 'profile' && snapshot}{@const profileApi = api}<ProfileSettings api={profileApi} user={snapshot.user} onSaved={user => { if (api === profileApi) profileSaved(user); }} onError={scopedError(api)}/>
      {:else if modal === 'pins' && snapshot && active}<PinsPanel bind:this={pinsPanel} {api} {snapshot} channel={active} onJump={jumpToMessage} onLink={confirmLink} onError={scopedError(api)}/>
      {:else if modal === 'threads' && snapshot && active}<ThreadList {api} {snapshot} parent={activeParent ?? active} onOpen={channel => { modal = null; void openChannel(channel); }} onError={scopedError(api)}/>
      {:else if modal === 'channel-permissions' && snapshot && channelSettings}<ChannelPermissions {api} {snapshot} channel={channelSettings} onError={scopedError(api)}/>
      {:else if modal === 'server-emojis' && snapshot}{@const emojiApi = api}<CustomEmojiSettings api={emojiApi} {snapshot} onChanged={emojis => { if (api === emojiApi && snapshot) snapshot = { ...snapshot, emojis }; }} onError={scopedError(api)}/>
      {:else if modal==='search'&&snapshot}<SearchPanel {api} {snapshot} {dms} onJump={jumpToMessage} onLink={confirmLink}/>{#if error}<p class="error" role="alert">{error}</p>{/if}
      {:else if modal==='appearance'}<Appearance color={themeColor} onChange={savedTheme}/>{:else if modal==='notifications'&&snapshot}{#if preferencesLoaded}<NotificationSettings {api} {preferences} device={deviceNotifications} channel={active} onSaved={savedPreferences} onDevice={savedDevice}/>{:else}<h2 id="dialog-title">Notifications &amp; messages</h2><p role="status">{socialLoading?'Loading your preferences…':socialError||'Preferences are unavailable.'}</p><button class="secondary" disabled={socialLoading} onclick={()=>loadSocial()}>Retry preferences</button>{/if}
      {:else if modal==='dm-picker'&&snapshot}<h2 id="dialog-title">New direct message</h2><p class="small muted">Choose a member of this community. DMs are not end-to-end encrypted; the host administrator can read them.</p><label>Find a member<input type="search" bind:value={dmQuery} placeholder="Name or username"/></label>{#each snapshot.members.filter(m=>m.user.id!==snapshot!.user.id&&(m.user.display_name.toLocaleLowerCase().includes(dmQuery.toLocaleLowerCase())||m.user.username.toLocaleLowerCase().includes(dmQuery.toLocaleLowerCase()))).slice(0,50) as member(member.user.id)}<button class="server-choice" disabled={busy||!can(snapshot.permissions,16,snapshot.is_owner)} onclick={()=>openDirect(member.user.id)}><span>{member.user.display_name} <span class="small muted">@{member.user.username}</span></span></button>{/each}{#if error}<p class="error" role="alert">{error}</p>{/if}
      {:else if modal === 'admin' && snapshot}<AdminPanel {api} {snapshot} onCreated={createdServer} onOpen={server=>{void chooseServer(server);}} onError={scopedError(api)}/>
      {:else if modal === 'ai-review' && snapshot && can(snapshot.permissions, 2, snapshot.is_owner)}<h2 id="dialog-title">AI review queue</h2><AiReview {api} {snapshot} latest={latestAiFlag} onUpdate={updateAiFlag} onMember={member => { selectedMember = member; modal = 'member'; }}/>
      {:else if modal === 'member' && snapshot && selectedMember}<MemberControls {api} {snapshot} member={selectedMember} onDirect={openDirect} onError={scopedError(api)} onClose={memberClose(selectedMember, api)}/>
      {:else if modal === 'media' && viewedAttachment}<h2 id="dialog-title">{viewedAttachment.filename}</h2>{#if viewedAttachment.kind === 'image'}<img class="lightbox-image" src={mediaUrl(viewedAttachment.url, origin)} alt={viewedAttachment.filename}/>{:else}<video class="lightbox-image" controls src={mediaUrl(viewedAttachment.url, origin)}><track kind="captions"/></video>{/if}<p class="small muted">If the media has expired, reopen the channel for a fresh link.</p>
      {:else if modal === 'channel-settings' && channelSettings}
        <h2 id="dialog-title">Channel settings</h2>
        <form onsubmit={saveChannel}>
          <label>Channel name<input bind:value={channelSettings.name} required maxlength="64"/></label><label>Topic<textarea bind:value={channelSettings.topic} maxlength="1024"></textarea></label><label>Category<select bind:value={channelSettings.category_id}><option value={null}>No category</option>{#each categories as category}<option value={category.id}>{category.name}</option>{/each}</select></label>
          {#if channelSettings.type === 'text'}<label class="check-label"><input type="checkbox" bind:checked={channelSettings.media_allowed}/>Allow images, videos, audio &amp; files</label><p class="small muted">Members also need the Attach files permission.</p>
          {:else}<label>Maximum voice participants<input type="number" min="1" max="99" step="1" required value={channelSettings.user_limit ?? 10} oninput={event => channelSettings!.user_limit = event.currentTarget.valueAsNumber}/></label><p class="small muted">Estimated host upload at capacity: {roomUploadMbps(channelSettings.user_limit ?? 10).toFixed(1)} Mbps if everyone talks.</p>{#if (channelSettings.user_limit ?? 10) > 15}<p class="notice">Large rooms need a fast upload on the host and can stutter.</p>{/if}{/if}
          {#if channelSettings.parent_id}<label class="check-label"><input type="checkbox" bind:checked={channelSettings.archived}/>Archive thread (new messages and uploads disabled)</label>{/if}
          {#if error}<div class="error" role="alert">{error}</div>{/if}<button class="primary" disabled={busy}>Save channel</button>
        </form><button class="secondary danger" disabled={busy} onclick={() => confirmChannelDelete = true}>Delete channel</button>{#if confirmChannelDelete}<div class="confirm-box"><p>Permanently delete #{channelSettings.name} and all its messages?</p><div class="button-row"><button class="secondary" disabled={busy} onclick={() => confirmChannelDelete = false}>Keep channel</button><button class="primary" disabled={busy} onclick={deleteChannel}>Confirm delete channel</button></div></div>{/if}
      {:else if modal === 'host-create' && hubHostServers && snapshot}
        <h2 id="dialog-title">Create a server</h2><HostServers {api} servers={hubHostServers} onCreated={createdServer} onOpen={server=>{void chooseServer(server);}} onError={scopedError(api)}/>
      {:else if modal === 'saved'}
        <SavedServers servers={savedServers} {busy} {error} onOpen={server=>{void connectTarget(server);}} onConnect={target=>{void connectTarget(target);}} onRemove={server=>{void removeSaved(server);}}/>
      {:else if modal === 'browse'}
        <DiscoveryBrowser {origin} onRemote={url=>{const target=parseServerLink(url);if(target)void connectTarget(target);}} onLocal={server=>{void chooseServer(server);}}/>{#if busy}<p role="status">Connecting to the selected host…</p>{/if}{#if error}<p class="error" role="alert">{error}</p>{/if}
      {:else if modal === 'servers'}
        <div class="brand-mark"><Icon name="chat" size={25}/></div><h2 id="dialog-title">Find your people.</h2><p>Choose a server on {host?.name ?? 'this host'}. Your account belongs to the server you join.</p><p class="current-host">Current host: <strong>{origin}</strong></p>
        {#if busy}<div class="loading" role="status">Connecting to host…</div>{/if}
        {#each host?.servers ?? [] as server}<button class="server-choice" disabled={busy} onclick={() => chooseServer(server)}><div class="avatar">{initials(server.name)}</div><div><strong>{server.name}</strong><p>{server.description || (server.invite_only ? 'Invite required' : 'Your next conversation starts here')}</p></div><Icon name={server.requires_password ? 'lock' : 'chevron'} size={17}/></button>{/each}
        {#if !busy && host && !host.servers.length}<p class="small muted">No public servers are listed. Enter a slug to find an unlisted server.</p>{/if}
        <form onsubmit={event => { event.preventDefault(); void findServer(); }}><label>Have an unlisted server slug?<input bind:value={slugInput} placeholder="my-community" pattern={'[a-z0-9\\-]{2,32}'} required maxlength="32"/></label><button class="secondary" disabled={busy} type="submit">Find server</button></form>
        {#if error}<div class="error" role="alert">{error}</div>{/if}
        <button class="load-older" style="margin:24px auto 0" onclick={() => { error = ''; hostInput = origin; modal = 'host'; }}>Connect to another host<Icon name="arrow" size={12}/></button>
      {:else if modal === 'host'}
        <div class="brand-mark"><Icon name="globe" size={24}/></div><h2 id="dialog-title">{isDesktopApp&&!host?'Welcome to ChitChat.':'A different home.'}</h2>{#if isDesktopApp&&!host}<p>ChitChat has no central server: each community lives on a host that someone runs. Enter the address of the host you were invited to, for example https://chat.example.com.</p>{:else}<p>Enter the address shared by your host. Use the full address, including https:// or http://.</p><p class="current-host">Current host: <strong>{origin}</strong></p>{/if}
        {#if origin !== location.origin&&!isDesktopApp}<button class="secondary return-host" disabled={busy} onclick={() => discover(location.origin)}>Back to {location.origin}</button>{/if}
        <form onsubmit={event => { event.preventDefault(); const target=parseServerLink(hostInput); if(target)void connectTarget(target); else void discover(hostInput); }}><label>Host address, server link or invite link<input type="url" bind:value={hostInput} placeholder="https://chat.example.com" required/></label>{#if error}<div class="error" role="alert">{error}</div>{/if}<button class="primary" disabled={busy}>{busy ? 'Connecting…' : 'Connect to host'}<Icon name="arrow" size={16}/></button>{#if host}<button type="button" class="load-older" style="margin-bottom:0" disabled={busy} onclick={() => { modal = 'servers'; error = ''; }}>Back to servers</button>{:else if savedServers.length}<button type="button" class="load-older" style="margin-bottom:0" disabled={busy} onclick={() => { modal = 'saved'; error = ''; }}>Your saved servers</button>{/if}</form>
      {:else if modal === 'auth'}
        <div class="brand-mark"><Icon name="chat" size={25}/></div><h2 id="dialog-title">{api.token ? 'Signing you back in…' : mode === 'login' ? 'Good to have you here.' : 'Make yourself at home.'}</h2><p>{selected?.name} · {new URL(origin).host}<br/>Your account stays with this community.</p>
        {#if api.token}<p class="notice" role="status">Restoring your saved sign-in. {status === 'reconnecting' ? 'Waiting for the host to reconnect…' : 'Connecting to live chat…'}</p><button class="secondary" onclick={startConnection}>Retry saved sign-in</button><button class="load-older" onclick={()=>{clearContext();modal='servers';error='';}}>Choose another server</button>
        {:else}
        {#if invitePreview}<div class="notice">You’re invited to {invitePreview.server.name}.{invitePreview.server.description ? ` ${invitePreview.server.description}` : ''}{invitePreview.expires_at ? ` Expires ${new Date(invitePreview.expires_at).toLocaleString()}.` : ''}{invitePreview.uses_left !== null ? ` ${invitePreview.uses_left} uses left.` : ''}</div>{/if}
        <div class="auth-tabs"><button class:active={mode === 'login'} disabled={busy} onclick={() => { mode = 'login'; error = ''; }}>Sign in</button><button class:active={mode === 'register'} disabled={busy} onclick={() => { mode = 'register'; error = ''; }}>Create account</button></div>
        <form onsubmit={authenticate}>
          <label>Username<input bind:value={username} autocomplete="username" required pattern={'[a-zA-Z0-9_.]{2,32}'} minlength="2" maxlength="32" placeholder="your_name"/></label>
          {#if mode === 'register'}<label>Display name<input bind:value={displayName} autocomplete="nickname" maxlength="32" placeholder="What should we call you?"/></label>{/if}
          <label>Password<input type="password" bind:value={password} autocomplete={mode === 'login' ? 'current-password' : 'new-password'} required minlength={mode === 'register' ? 8 : undefined} maxlength="128" placeholder={mode === 'register' ? 'At least 8 characters' : 'Your server password'}/></label>
          {#if mode === 'register'}
            {#if !selected?.registration_open}<div class="notice">Registration may be closed. The owner can use the first-account setup code below.</div>{/if}
            {#if selected?.requires_password}<label>Community access password<input type="password" bind:value={serverPassword} autocomplete="off"/></label>{/if}
            <label>Invite code {selected?.invite_only ? '(required)' : '(optional)'}<input bind:value={inviteCode} autocomplete="off" required={selected?.invite_only && !setupCode}/></label>
            <button type="button" class="small muted" aria-expanded={showSetup} onclick={() => showSetup = !showSetup}>Setting up your own server?</button>
            {#if showSetup}<label>Owner setup code<input bind:value={setupCode} autocomplete="off" placeholder="From the host console"/></label>{/if}
          {/if}
          <label class="check-label"><input type="checkbox" bind:checked={keepSignedIn}/>Keep me signed in</label><p class="small muted">Turn this off on a shared device. You can sign out at any time.</p>
          {#if error}<div class="error" role="alert">{error}</div>{/if}
          {#if status === 'connecting' || status === 'reconnecting'}<div class="notice" role="status">Connecting to live chat…</div>{/if}
          <button class="primary" type="submit" disabled={busy || status === 'connecting' || status === 'reconnecting'}>{busy ? 'One moment…' : mode === 'login' ? 'Sign in to server' : 'Create your account'}<Icon name="arrow" size={16}/></button>
        </form>
        <p class="footnote"><Icon name="lock" size={10}/> Your host manages your account. There is no email recovery.</p>
        <button class="load-older" style="margin:18px auto 0" disabled={busy} onclick={() => { clearContext(); modal = 'servers'; error = ''; }}>Choose another server</button>
        {/if}
      {:else if modal === 'channel'}
        <h2 id="dialog-title">Start a conversation.</h2><p>Create a channel for your community.</p><form onsubmit={createChannel}><label>Channel name<input bind:value={channelName} placeholder="general" required maxlength="64"/></label><label>Type<select bind:value={channelType}><option value="text">Text</option><option value="voice">Voice</option></select></label>{#if channelType === 'voice'}<label>Maximum voice participants<input type="number" min="1" max="99" step="1" required bind:value={channelLimit}/></label><p class="small muted">Estimated host upload at capacity: {roomUploadMbps(channelLimit).toFixed(1)} Mbps.</p>{#if channelLimit > 15}<p class="notice">Large rooms need a fast upload on the host and can stutter.</p>{/if}{/if}<label>Topic (optional)<textarea bind:value={channelTopic} placeholder="What is this space for?" maxlength="1024"></textarea></label><label>Category<select bind:value={channelCategory}><option value="">No category</option>{#each categories as category}<option value={category.id}>{category.name}</option>{/each}</select></label>{#if error}<div class="error" role="alert">{error}</div>{/if}<button class="primary" disabled={busy}>{busy ? 'Creating…' : 'Create channel'}</button></form>
      {:else if modal === 'link'}
        <h2 id="dialog-title">Open this link?</h2><p>You’re about to visit <strong>{new URL(pendingLink).hostname}</strong> in a new tab. Check the address before continuing.</p>
        <div class="link-destination">{pendingLink}</div>
        {#if new URL(pendingLink).protocol === 'http:'}<p class="small danger" style="margin-top:12px">This destination uses an unencrypted HTTP connection.</p>{/if}
        {#if linkTarget}<p class="small muted" style="margin-top:12px">This is a ChitChat {linkTarget.invite?'invite':'community'} link ({linkTarget.slug}).</p>{#if error}<div class="error" role="alert">{error}</div>{/if}<div class="confirm-actions"><a class="secondary" href={pendingLink} target="_blank" rel="noopener noreferrer" onclick={() => modal = null}>Open in browser</a><button class="primary" disabled={busy} onclick={() => { const target = linkTarget; if (target) void connectTarget(target); }}>{busy ? 'Connecting…' : 'Open in ChitChat'}<Icon name="arrow" size={16}/></button></div>{:else}<div class="confirm-actions"><button class="secondary" onclick={closeModal}>Stay here</button><a class="primary" href={pendingLink} target="_blank" rel="noopener noreferrer" onclick={() => modal = null}>Open link<Icon name="arrow" size={16}/></a></div>{/if}
      {:else if modal === 'delete'}
        <h2 id="dialog-title">Delete this message?</h2><p>This will permanently remove the message for everyone in the channel.</p><p class="message-text">{deleting?.content}</p>{#if error}<div class="error" role="alert">{error}</div>{/if}<div class="confirm-actions"><button class="secondary" disabled={actionBusy} onclick={closeModal}>Keep message</button><button class="primary" style="background:var(--c-ffb6b0, #ffb6b0)" disabled={actionBusy} onclick={deleteMessage}>{actionBusy ? 'Deleting…' : 'Delete message'}</button></div>
      {/if}
    </div>
  </div>
  {/key}
{/if}
{#if toast}<div class="toast notice" role="status">{toast}</div>{/if}
