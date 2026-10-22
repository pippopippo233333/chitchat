import type { Member, Role, Snapshot } from './api';
import { can } from './api';
export const permissionNames = ['View channels', 'Send messages', 'Manage messages', 'Manage channels', 'Manage roles', 'Kick / timeout members', 'Ban members', 'Attach files', 'Add reactions', 'Administrator', 'Manage server', 'Create invites', 'Connect to voice', 'Speak in voice', 'Share screen'];
export function highestRole(member: Member | undefined, roles: Role[]) { return Math.max(0, ...roles.filter(role => role.is_default || member?.role_ids.includes(role.id)).map(role => role.position)); }
export function canManageRole(snapshot: Snapshot, role: Role) {
  return can(snapshot.permissions, 4, snapshot.is_owner) && (snapshot.is_owner || role.position < highestRole(snapshot.members.find(member => member.user.id === snapshot.user.id), snapshot.roles));
}
export function canModerate(snapshot: Snapshot, target: Member) {
  return !target.is_owner && target.user.id !== snapshot.user.id && (snapshot.is_owner || highestRole(target, snapshot.roles) < highestRole(snapshot.members.find(member => member.user.id === snapshot.user.id), snapshot.roles));
}
export function togglePermission(value: string, bit: number, enabled: boolean) { const mask = 1n << BigInt(bit); return (enabled ? BigInt(value) | mask : BigInt(value) & ~mask).toString(); }
