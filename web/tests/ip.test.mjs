import test from 'node:test';
import assert from 'node:assert/strict';
import {allocation,subnetInfo} from '../src/ip.mjs';
test('/28 allocation excludes network and broadcast and reserves gateway once',()=>{
 const a=allocation({cidr:'10.0.3.80/28',gateway:'10.0.3.81'},[{ip:'10.0.3.81'},{ip:'10.0.3.83'}]);
 assert.equal(a.total,14);assert.equal(a.used,2);assert.equal(a.free,12);assert.equal(a.next,'10.0.3.82');
});
test('/31 and /32 include every address',()=>{assert.equal(subnetInfo('10.0.0.0/31').total,2);assert.equal(allocation({cidr:'10.0.0.1/32',gateway:''},[]).next,'10.0.0.1')});
test('/0 stays bounded and numeric',()=>{const a=allocation({cidr:'0.0.0.0/0',gateway:''},[]);assert.equal(a.total,4294967294);assert.equal(a.next,'0.0.0.1')});
test('full subnet returns no next IP',()=>{const a=allocation({cidr:'10.0.0.1/32',gateway:'10.0.0.1'},[]);assert.equal(a.next,null);assert.equal(a.free,0)});
