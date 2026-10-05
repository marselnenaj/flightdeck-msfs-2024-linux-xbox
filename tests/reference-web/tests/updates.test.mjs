import test from 'node:test';
import assert from 'node:assert/strict';
import {normalizeUpdate,updateActions,updateTitle} from '../updates.js';
import {normalizeSetup,setupBusy} from '../setup.js';
import {setLanguage} from '../i18n.js';

const raw=()=>({available:true,installed_version:'1.8.16.0',latest_version:'1.9.0.0',update_available:true,can_check:true,can_start:true,can_rollback:true,auth_required:false,job:{id:'synthetic-update',mode:'update',state:'ready',phase:'package_check',checks:[]}});
const status=()=>({runtime:{configured:true},game:{state:'stopped'},csrf_token:'fixture-csrf'});
const opts=()=>({online:true,fresh:true});
test('background discovery shows progress and availability without download capability',()=>{
  const checking=normalizeUpdate({...raw(),job:null,can_start:false,background_checking:true});
  setLanguage('en');assert.equal(updateTitle(checking),'Checking MSFS version');
  assert.equal(updateActions(checking,status(),opts()).start,false);
  assert.equal(updateActions({...checking,background_checking:false},status(),opts()).check,true);
  setLanguage('de');
});
test('versions and availability come only from typed backend fields',()=>{
  assert.throws(()=>normalizeUpdate({}));
  const d=normalizeUpdate({...raw(),installed_version:42,latest_version:'',update_available:'false',can_start:'true'});
  assert.equal(d.installed_version,null);assert.equal(d.latest_version,null);assert.equal(d.update_available,null);assert.equal(d.can_start,false);
  assert.equal(normalizeUpdate({...raw(),latest_version:'x'.repeat(500)}).latest_version.length,128);
  assert.equal(normalizeUpdate({...raw(),job:{id:'setup',mode:'install',state:'ready'}}).job,null);
});
test('ready update requires real versions, exact job and idle fresh status',()=>{
  const d=normalizeUpdate(raw());assert.equal(updateActions(d,status(),opts()).start,true);
  assert.equal(setupBusy(d.job),false);assert.equal(updateActions(d,status(),opts()).check,true);
  assert.equal(setupBusy({...d.job,mode:'install'}),true);assert.equal(setupBusy({...d.job,state:'installing'}),true);
  for(const option of [{online:false},{fresh:false},{pending:true},{reserved:true,setupJob:{id:'other'}}])assert.equal(updateActions(d,status(),{...opts(),...option}).start,false);
  for(const state of ['running','starting','stopping','external','unknown'])assert.equal(updateActions(d,{...status(),game:{state}},opts()).start,false);
  assert.equal(updateActions(d,status(),{...opts(),reserved:true,setupJob:{id:d.job.id}}).start,true);
  for(const delta of [{latest_version:null},{installed_version:null},{update_available:null},{can_start:false},{job:null}])assert.equal(updateActions({...d,...delta},status(),opts()).start,false);
  assert.equal(updateActions(d,{...status(),csrf_token:''},opts()).start,false);
});
test('checking and signing in are distinct explicit actions',()=>{
  const d=normalizeUpdate({...raw(),job:null});
  assert.equal(updateActions(d,status(),opts()).check,true);assert.equal(updateActions(d,status(),opts()).signIn,false);
  d.auth_required=true;assert.equal(updateActions(d,status(),opts()).check,false);assert.equal(updateActions(d,status(),opts()).signIn,true);
  d.available=false;assert.equal(updateActions(d,status(),opts()).signIn,false);
});
test('update jobs share genuine pause/resume capability and cancellation',()=>{
  const d=normalizeUpdate({...raw(),job:{...raw().job,state:'installing',phase:'download',can_pause:true}});
  assert.equal(normalizeSetup({available:true,job:raw().job}).job.mode,'update');
  assert.equal(updateActions(d,status(),opts()).pause,true);assert.equal(updateActions(d,status(),opts()).check,false);
  d.job.phase='pausing';assert.equal(updateActions(d,status(),opts()).pause,false);assert.equal(updateActions(d,status(),opts()).resume,false);
  d.job.phase='paused';d.job.can_resume=true;assert.equal(updateActions(d,status(),opts()).resume,true);assert.equal(updateActions(d,status(),opts()).cancel,true);
  d.job.state='complete';assert.equal(updateActions(d,status(),opts()).resume,false);assert.equal(updateActions(d,status(),opts()).cancel,false);
});
test('rollback requires actual capability and cannot overlap an update',()=>{
  const d=normalizeUpdate({...raw(),job:{...raw().job,state:'checking'}});assert.equal(updateActions(d,status(),opts()).rollback,false);
  d.job=null;assert.equal(updateActions(d,status(),opts()).rollback,true);
  d.available=false;assert.equal(updateActions(d,status(),opts()).rollback,true);
  d.can_rollback=false;assert.equal(updateActions(d,status(),opts()).rollback,false);
});
test('unknown status is never translated into an up-to-date claim',()=>{
  setLanguage('de');const d=normalizeUpdate({...raw(),job:null,update_available:null});assert.match(updateTitle(d),/Noch nicht/);
  d.update_available=false;assert.match(updateTitle(d),/aktuell/);
  setLanguage('en');assert.equal(updateTitle(d),'Your game version is up to date');
  d.auth_required=true;assert.equal(updateTitle(d),'Sign-in required to check');
  d.available=false;assert.equal(updateTitle(d),'Updates currently unavailable');setLanguage('de');
});

test('integrity counters are typed backend values, never guessed from update success',()=>{
  const result={checked:50,missing:1,changed:2,unreadable:3,total:50,healthy:false};
  const d=normalizeUpdate({...raw(),integrity:{available:true,can_check:true,result}});
  assert.deepEqual(d.integrity.result,result);
  assert.equal(normalizeUpdate({...raw(),integrity:{available:true,result:{...result,checked:'50'}}}).integrity.result,null);
  assert.equal(normalizeUpdate(raw()).integrity.available,false);
  assert.equal(normalizeUpdate({...raw(),integrity:{result:{...result,total:Infinity}}}).integrity.result,null);
});
test('local file check is independent of downloader but requires idle actual capability',()=>{
  const d=normalizeUpdate({...raw(),available:false,job:null,integrity:{available:true,can_check:true}});
  assert.equal(updateActions(d,status(),opts()).verify,true);
  assert.equal(updateActions(d,{...status(),game:{state:'running'}},opts()).verify,false);
  d.integrity.can_check=false;assert.equal(updateActions(d,status(),opts()).verify,false);
});
test('repair explicitly permits same-version replacement and retains operation on sign-in failure',()=>{
  const d=normalizeUpdate({...raw(),latest_version:'1.8.16.0',update_available:false,can_repair:true,job:{...raw().job,operation:'repair'}});
  assert.equal(d.job.operation,'repair');assert.equal(updateActions(d,status(),opts()).start,true);
  assert.equal(updateTitle(d),'Reparatur vorbereitet');
  d.job.state='failed';d.auth_required=true;d.can_check=false;
  assert.equal(updateActions(d,status(),opts()).signIn,true);assert.equal(updateActions(d,status(),opts()).repair,false);
  d.auth_required=false;assert.equal(updateActions(d,status(),opts()).repair,true);
  d.can_repair=false;assert.equal(updateActions(d,status(),opts()).repair,false);
});
