import { describe, expect, it } from 'vitest';
import type { Snapshot, Member, Role } from './api';
import { canModerate, canManageRole, togglePermission } from './permissions';
const role = (id: string, position: number): Role => ({ id, position, name: id, color: '#ffffff', permissions: '0', is_default: id === 'everyone' });
const member = (id: string, role_ids: string[], is_owner = false) => ({ user: { id }, role_ids, is_owner }) as Member;
describe('administrative permission and rank controls', () => {
  const roles = [role('everyone', 0), role('helper', 1), role('mod', 2)];
  const snapshot = { user: { id: 'me' }, permissions: '112', is_owner: false, roles, members: [member('me', ['mod'])] } as Snapshot;
  it('only allows management of roles below your highest rank', () => { expect(canManageRole(snapshot, roles[1])).toBe(true); expect(canManageRole(snapshot, roles[2])).toBe(false); });
  it('protects the owner, yourself and equally ranked members from moderation', () => {
    expect(canModerate(snapshot, member('other', ['helper']))).toBe(true);
    expect(canModerate(snapshot, member('other', ['mod']))).toBe(false);
    expect(canModerate(snapshot, member('owner', [], true))).toBe(false);
    expect(canModerate({ ...snapshot, is_owner: true }, member('me', []))).toBe(false);
  });
  it('toggles all permission bits without destroying high bits', () => {
    expect(togglePermission('9223372036854775808', 14, true)).toBe('9223372036854792192');
    expect(togglePermission('259', 8, false)).toBe('3');
  });
});
