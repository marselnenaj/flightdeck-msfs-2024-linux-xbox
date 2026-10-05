import assert from 'node:assert/strict';
import test from 'node:test';
import {gsxPermissions,gsxProgress} from '../gsx.js';

const status={runtime:{path:'/fixture'},game:{state:'stopped'}};
const data={state:'available',runtime_path:'/fixture',can_change:true,prepared:false,package_installed:false,startup_found:false,configured:false};
test('GSX gates preparation, official installation and startup on separate evidence',()=>{
  assert.deepEqual(gsxPermissions(data,status,true),{prepare:true,open:false,configure:false,disable:false,recover:false,stop:false});
  const prepared={...data,prepared:true,job:{state:'complete',operation:'prepare'}};
  assert.equal(gsxProgress(prepared).step,2);
  assert.equal(gsxPermissions(prepared,status,true).configure,false);
  assert.equal(gsxPermissions({...prepared,package_installed:true},status,true).configure,false);
  const installed={...prepared,package_installed:true,startup_found:true};
  assert.equal(gsxPermissions(installed,status,true).configure,true);
  assert.equal(gsxProgress({...installed,configured:true}).title,'GSX eingerichtet · Flugtest ausstehend');
});
test('GSX refuses stale, offline, busy and running game operations',()=>{
  for(const permissions of [gsxPermissions(data,status,false),gsxPermissions({...data,runtime_path:'/old'},status,true),
    gsxPermissions({...data,can_change:false},status,true),gsxPermissions(data,{...status,game:{state:'running'}},true)]) {
    assert.ok(Object.values(permissions).every(value=>!value));
  }
  const busy={...data,can_change:false,can_stop:true,job:{state:'running',operation:'open'}};
  assert.equal(gsxPermissions(busy,{...status,game:{state:'external'}},true).stop,true);
  assert.equal(gsxPermissions({...busy,runtime_path:'/other'},status,true).stop,false);
});
test('interrupted GSX setup offers recovery and never an installer or game-ready claim',()=>{
  const interrupted={...data,state:'interrupted',can_recover:true};
  assert.equal(gsxPermissions(interrupted,status,true).recover,true);
  assert.equal(gsxPermissions(interrupted,status,true).prepare,false);
  assert.match(gsxProgress(interrupted).title,/unterbrochen/);
  assert.equal(gsxProgress({...data,job:{state:'complete',operation:'open'}}).step,1);
});
