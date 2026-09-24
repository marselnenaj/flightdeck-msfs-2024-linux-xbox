import assert from 'node:assert/strict';
import test from 'node:test';
import {fenixPermissions,fenixProgress} from '../fenix.js';

test('Fenix actions require a current idle runtime and reconcile permissions',()=>{
  const status={runtime:{path:'/fixture'},game:{state:'stopped'}};
  const value={state:'available',runtime_path:'/fixture',can_change:true};
  assert.equal(fenixPermissions(value,status,true).install,true);
  for(const permissions of [fenixPermissions(value,status,false),fenixPermissions({...value,runtime_path:'/other'},status,true),
    fenixPermissions(value,{...status,game:{state:'running'}},true),fenixPermissions({...value,can_change:false},status,true)]){
    assert.ok(Object.values(permissions).every(value=>!value));
  }
  const installed={...value,state:'installed',installed:true,fenix_installed:true,settings_ready:true,manager_installed:true,can_restore:true};
  assert.deepEqual(fenixPermissions(installed,status,true),{install:false,installer:true,open:true,manager:true,configure:true,restore:true,stop:false});
  assert.equal(fenixPermissions({...installed,update_available:true},status,true).install,true);
  assert.equal(fenixPermissions({...installed,update_available:true,can_change:false},status,true).install,false);
  assert.deepEqual(fenixPermissions({...value,state:'legacy',manager_installed:true},status,true),{install:false,installer:false,open:false,manager:true,configure:false,restore:false,stop:false});
  assert.equal(fenixPermissions({...value,state:'legacy',fenix_installed:true},status,true).open,true);
  assert.equal(fenixPermissions({...value,state:'committing',can_restore:true},status,true).restore,true);
});

test('Fenix completion follows setup evidence, not a completed installer job',()=>{
  const status={game:{state:'stopped'}};
  const available={state:'available',idle:true};
  const patched={...available,state:'installed',installed:true,configured:false,job:{state:'complete',operation:'installer'}};
  assert.equal(fenixProgress(available,status).step,1);
  assert.equal(fenixProgress(patched,status).step,2);
  const aircraft={...patched,fenix_installed:true};
  assert.equal(fenixProgress(aircraft,status).step,3);
  const settings={...aircraft,settings_ready:true};
  assert.equal(fenixProgress(settings,status).step,4);
  assert.equal(fenixProgress(settings,status).ready,false);
  const ready={...settings,configured:true};
  assert.equal(fenixProgress(ready,status).ready,true);
  assert.deepEqual(fenixProgress(ready,status).steps,['done','done','done','done']);
  assert.equal(fenixProgress({...ready,settings_ready:false},status).ready,false);
  assert.equal(fenixProgress({...ready,fenix_installed:false},status).ready,false);
  assert.equal(fenixProgress({...ready,job:{state:'running',operation:'configure'}},status).ready,false);
  assert.equal(fenixProgress({...ready,state:'legacy'},status).ready,false);
});

test('open Wine applications explain a disabled step after the installer exits',()=>{
  const status={runtime:{path:'/fixture'},game:{state:'stopped'}};
  const value={state:'installed',installed:true,fenix_installed:true,settings_ready:false,runtime_path:'/fixture',
    idle:false,can_change:false,job:{state:'complete',operation:'installer'}};
  assert.match(fenixProgress(value,status).busy,/Windows-Anwendung/);
  assert.ok(Object.values(fenixPermissions(value,status,true)).every(value=>!value));
  assert.equal(fenixPermissions({...value,can_change:true,idle:true},status,true).configure,false);
  assert.match(fenixProgress(value,{game:{state:'running'}}).busy,/MSFS läuft/);
  assert.equal(fenixProgress({...value,idle:true},status).busy,'');
  const external={...status,game:{state:'external'}};
  assert.match(fenixProgress({...value,job:{state:'running',operation:'open'}},external).busy,/Windows-Anwendung/);
  assert.equal(fenixProgress({...value,job:{state:'running',operation:'install'}},external).busy,'');
  assert.match(fenixProgress({...value,job:null},external).busy,/Installation wird gerade verwendet/);
  assert.equal(fenixPermissions({...value,can_stop:true},external,true).stop,true);
  assert.equal(fenixPermissions({...value,can_stop:false},external,true).stop,false);
  assert.equal(fenixPermissions({...value,can_stop:true,runtime_path:'/other'},external,true).stop,false);
  assert.equal(fenixProgress({...value,job:{state:'running',operation:'open',stopping:true}},external).title,'Fenix wird beendet …');
});
