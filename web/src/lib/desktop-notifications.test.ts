import{describe,it,expect}from'vitest';import{knownNotificationTarget}from'./desktop-notifications';import type{SavedServer}from'./saved-servers';
const saved=[{origin:'https://chat.example',slug:'main',name:'Community'}]as SavedServer[];
describe('notification navigation',()=>{
 it('only reconnects communities the user already saved',()=>{expect(knownNotificationTarget({origin:'https://chat.example',slug:'main',channel_id:'123'},saved)).toEqual({origin:'https://chat.example',slug:'main',channel_id:'123'});expect(knownNotificationTarget({origin:'https://other.example',slug:'main'},saved)).toBeNull();});
 it('rejects credentials, script schemes, paths and invalid channel ids',()=>{for(const target of [{origin:'javascript:alert(1)',slug:'main'},{origin:'https://x@chat.example',slug:'main'},{origin:'https://chat.example/path',slug:'main'},{origin:'https://chat.example',slug:'../main'},{origin:'https://chat.example',slug:'main',channel_id:'x'}])expect(knownNotificationTarget(target,saved)).toBeNull();});
});
