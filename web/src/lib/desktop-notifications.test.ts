import{describe,it,expect}from'vitest';import{knownNotificationTarget}from'./desktop-notifications';import type{SavedServer}from'./saved-servers';
const saved=[{origin:'https://chat.example',slug:'main',name:'Community'}]as SavedServer[];
describe('notification navigation',()=>{
 it('keeps a message target only with its validated conversation',()=>{
  const target={origin:'https://chat.example',slug:'main',channel_id:'123',message_id:'9007199254740993'};
  expect(knownNotificationTarget(target,saved)).toEqual(target);
  for(const invalid of [{...target,message_id:'1/../../x'},{...target,message_id:123},{...target,message_id:'1'.repeat(20)},{origin:target.origin,slug:'main',message_id:'123'}])expect(knownNotificationTarget(invalid,saved)).toBeNull();
 });
 it('only reconnects communities the user already saved',()=>{expect(knownNotificationTarget({origin:'https://chat.example',slug:'main',channel_id:'123'},saved)).toEqual({origin:'https://chat.example',slug:'main',channel_id:'123'});expect(knownNotificationTarget({origin:'https://other.example',slug:'main'},saved)).toBeNull();});
 it('rejects credentials, script schemes, paths and invalid channel ids',()=>{for(const target of [{origin:'javascript:alert(1)',slug:'main'},{origin:'https://x@chat.example',slug:'main'},{origin:'https://chat.example/path',slug:'main'},{origin:'https://chat.example',slug:'../main'},{origin:'https://chat.example',slug:'main',channel_id:'x'}])expect(knownNotificationTarget(target,saved)).toBeNull();});
});
