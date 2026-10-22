import {describe,it,expect} from 'vitest';import {suggestSlug,validSlug} from './host-servers';
describe('host server addresses',()=>{
 it('suggests lowercase ASCII addresses from names',()=>{expect(suggestSlug('Café & Friends!')).toBe('cafe-friends');expect(suggestSlug(' A '.repeat(40)).length).toBeLessThanOrEqual(32);expect(suggestSlug('😀')).toBe('');});
 it('validates the server address contract',()=>{for(const value of ['ab','friends-2','a'.repeat(32)])expect(validSlug(value)).toBe(true);for(const value of ['a','Upper','with space','../x','a'.repeat(33),''])expect(validSlug(value)).toBe(false);});
});
