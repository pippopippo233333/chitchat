// Emoji offered by the reaction picker, with search words. Any other emoji still works: the host
// accepts every short emoji string, and existing reactions show whatever was used.
export type EmojiEntry = { emoji: string; words: string; url?: string };
export type EmojiGroup = { name: string; items: EmojiEntry[] };

const raw: [string, string][] = [
  ['Smileys', '😀 grin smile happy|😃 smile happy|😄 smile laugh|😁 grin teeth|😆 laugh squint|😅 sweat laugh|🤣 rofl rolling laugh|😂 joy tears laugh|🙂 slight smile|🙃 upside down|😉 wink|😊 blush smile|😇 angel halo|🥰 love hearts|😍 heart eyes love|🤩 star struck|😘 kiss|😋 yum tongue|😛 tongue|😜 wink tongue crazy|🤪 zany crazy|🤑 money|🤗 hug|🤭 oops hand|🤫 shush quiet|🤔 think hmm|🤐 zip mouth|🤨 raised eyebrow|😐 neutral|😑 expressionless|😶 no mouth|😏 smirk|😒 unamused|🙄 eye roll|😬 grimace|😌 relieved|😔 pensive sad|😪 sleepy|😴 sleep zzz|😷 mask sick|🤒 thermometer sick|🤢 nauseated|🤮 vomit|🥵 hot|🥶 cold|😵 dizzy|🤯 mind blown exploding|🥳 party celebrate|😎 cool sunglasses|🤓 nerd|🧐 monocle|😕 confused|😟 worried|🙁 frown|😮 wow open mouth surprised|😯 hushed|😲 astonished|😳 flushed|🥺 pleading puppy|😦 frowning|😨 fearful|😰 anxious sweat|😥 sad relieved|😢 cry sad tear|😭 sob cry|😱 scream|😖 confounded|😣 persevere|😞 disappointed|😓 downcast sweat|😩 weary|😫 tired|🥱 yawn|😤 triumph huff|😡 angry rage|😠 angry mad|🤬 swear cursing|😈 devil smile|💀 skull dead|💩 poop|🤡 clown|👻 ghost|👽 alien|🤖 robot'],
  ['Gestures', '👍 thumbs up yes like|👎 thumbs down no dislike|👌 ok perfect|🤌 pinched fingers|✌️ peace victory|🤞 fingers crossed luck|🤟 love you|🤘 rock horns|🤙 call me|👈 point left|👉 point right|👆 point up|👇 point down|☝️ index up|✋ hand stop|👋 wave hello bye|🖐️ hand fingers|🖖 vulcan|👏 clap applause|🙌 raised hands hooray|👐 open hands|🤲 palms up|🤝 handshake deal|🙏 pray please thanks|✍️ writing|💪 muscle strong flex|🫡 salute|🫶 heart hands|👀 eyes look|🧠 brain smart|👂 ear listen|🫠 melting'],
  ['Hearts', '❤️ red heart love|🧡 orange heart|💛 yellow heart|💚 green heart|💙 blue heart|💜 purple heart|🖤 black heart|🤍 white heart|🤎 brown heart|💔 broken heart|❣️ heart exclamation|💕 two hearts|💞 revolving hearts|💓 beating heart|💗 growing heart|💖 sparkling heart|💘 cupid arrow heart|💝 heart gift|💯 hundred perfect|💢 anger|💥 boom collision|💫 dizzy star|💦 sweat drops|💨 dash fast|💬 speech bubble|💤 zzz sleep'],
  ['Nature & food', '🐶 dog|🐱 cat|🐭 mouse|🐰 rabbit bunny|🦊 fox|🐻 bear|🐼 panda|🐨 koala|🐯 tiger|🦁 lion|🐮 cow|🐷 pig|🐸 frog|🐵 monkey|🙈 see no evil monkey|🐔 chicken|🐧 penguin|🐦 bird|🦄 unicorn|🐝 bee|🦋 butterfly|🐢 turtle|🐍 snake|🐙 octopus|🐬 dolphin|🐳 whale|🌵 cactus|🌲 tree|🍀 clover luck|🌸 blossom flower|🌹 rose|🌻 sunflower|🌞 sun|🌙 moon|⭐ star|🌈 rainbow|🔥 fire lit hot|❄️ snow|⚡ lightning zap|🌊 wave water|🍎 apple|🍌 banana|🍉 watermelon|🍓 strawberry|🍕 pizza|🍔 burger|🍟 fries|🌮 taco|🍣 sushi|🍩 donut|🍪 cookie|🎂 cake birthday|🍿 popcorn|☕ coffee|🍺 beer|🍷 wine|🥤 drink'],
  ['Activities & things', '🎉 tada party celebrate|🎊 confetti|🎈 balloon|🎁 gift present|🏆 trophy win|🥇 gold medal first|⚽ soccer football|🏀 basketball|🎮 video game controller|🕹️ joystick|🎲 dice|🎯 target bullseye|🎵 music note|🎶 music notes|🎤 microphone sing|🎧 headphones|🎬 movie clapper|📸 camera|💻 laptop computer|📱 phone|⌨️ keyboard|🖱️ mouse computer|💡 idea bulb|📌 pin|📎 paperclip|🔒 lock|🔑 key|🔨 hammer|🛠️ tools|⚙️ gear settings|🧪 test tube|📈 chart up|📉 chart down|📅 calendar|⏰ alarm clock|⌛ hourglass|💰 money bag|💎 gem diamond|🚀 rocket launch|✈️ plane|🚗 car|🏠 house home|🗿 moai'],
  ['Symbols', '✅ check done yes|☑️ check box|✔️ check mark|❌ cross no wrong|❎ cross box|❓ question|❗ exclamation|‼️ double exclamation|⚠️ warning|🚫 prohibited no|⛔ no entry|🆗 ok button|🆕 new|🆒 cool button|🔴 red circle|🟠 orange circle|🟡 yellow circle|🟢 green circle|🔵 blue circle|🟣 purple circle|⚫ black circle|⚪ white circle|➕ plus|➖ minus|➡️ right arrow|⬅️ left arrow|⬆️ up arrow|⬇️ down arrow|🔁 repeat|🔄 refresh|♻️ recycle|✨ sparkles|❤️‍🔥 heart fire|👑 crown|🏳️‍🌈 rainbow flag|🏁 finish flag|🚩 red flag'],
];

export const EMOJI_GROUPS: EmojiGroup[] = raw.map(([name, list]) => ({
  name,
  items: list.split('|').map(item => { const space = item.indexOf(' '); return { emoji: item.slice(0, space), words: item.slice(space + 1) }; }),
}));

export const DEFAULT_RECENT = ['👍', '❤️', '😂', '😮', '😢', '🎉', '🔥', '👀'];
const RECENT_KEY = 'chitchat.recentEmoji';

export function loadRecent(): string[] {
  try {
    const saved = JSON.parse(localStorage.getItem(RECENT_KEY) ?? 'null');
    if (Array.isArray(saved) && saved.every(item => typeof item === 'string')) return saved.slice(0, 16);
  } catch { /* storage unavailable or corrupt */ }
  return DEFAULT_RECENT;
}

export function rememberEmoji(emoji: string): string[] {
  const next = [emoji, ...loadRecent().filter(item => item !== emoji)].slice(0, 16);
  try { localStorage.setItem(RECENT_KEY, JSON.stringify(next)); } catch { /* storage unavailable */ }
  return next;
}

export function searchEmoji(query: string): EmojiEntry[] {
  const words = query.toLocaleLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  return EMOJI_GROUPS.flatMap(group => group.items).filter(item => words.every(word => item.words.split(' ').some(w => w.startsWith(word)) || item.emoji === word));
}
