import test from 'node:test';
import assert from 'node:assert/strict';
import {normalizeSetup,setupBusy,regionName,normalizeRegion,regionOptions,installationHelp,downloadActions} from '../setup.js';
import {setLanguage} from '../i18n.js';
const base=()=>({available:true,prepare_available:true,directory_picker:false,job:null,defaults:{mode:'existing',local_saves:false}});
test('idle setup has no implied operation or consent',()=>{const s=normalizeSetup(base());assert.equal(s.job,null);assert.equal(setupBusy(s.job),false);assert.equal(s.defaults.local_saves,false);assert.equal(s.directory_picker,false);});
test('unknown or malformed jobs fail closed',()=>{assert.throws(()=>normalizeSetup(null));assert.throws(()=>normalizeSetup({available:'true'}));for(const job of [{id:'x',state:'mystery'},{id:1,state:'ready'},{state:'ready'}])assert.throws(()=>normalizeSetup({...base(),job}));});
test('ready reserves the launcher until commit or cancel',()=>{for(const state of ['checking','ready','installing'])assert.equal(setupBusy({state}),true);for(const state of ['complete','failed','cancelled'])assert.equal(setupBusy({state}),false);});
test('progress stays indeterminate unless a real bounded number exists',()=>{for(const progress of [null,undefined,-1,101,Infinity,'50'])assert.equal(normalizeSetup({...base(),job:{id:'x',state:'installing',progress}}).job.progress,null);assert.equal(normalizeSetup({...base(),job:{id:'x',state:'installing',progress:42}}).job.progress,42);});
test('setup text and checks are bounded without HTML interpretation',()=>{const s=normalizeSetup({...base(),job:{id:'x',state:'failed',error:'<img src=x>'+ 'a'.repeat(2000),checks:[{label:'<script>',ok:'true'}]}});assert.equal(s.job.error.length,1200);assert.equal(s.job.checks[0].label,'<script>');assert.equal(s.job.checks[0].ok,null);});

test('installation support is explicit and job mode survives normalization',()=>{assert.equal(normalizeSetup(base()).install_available,false);assert.equal(normalizeSetup({...base(),install_available:'true'}).install_available,false);assert.equal(normalizeSetup({...base(),install_available:true}).install_available,true);for(const mode of ['install','existing','prepare'])assert.equal(normalizeSetup({...base(),job:{id:'x',state:'installing',mode}}).job.mode,mode);assert.equal(normalizeSetup({...base(),job:{id:'x',state:'installing',mode:'unknown'}}).job.mode,null);});

test('region summary uses the selected language without inventing a missing region',()=>{
  setLanguage('de');assert.equal(regionName('at'),'Österreich (AT)');
  assert.equal(regionName(''),'Region auswählen …');
  assert.equal(regionName('invalid'),'Region auswählen …');
  setLanguage('en');assert.equal(regionName('AT'),'Austria (AT)');
  assert.equal(regionName(''),'Choose a region …');setLanguage('de');
});
test('region choices use ISO codes and localized names without treating unknown defaults as US',()=>{
  assert.equal(normalizeRegion(' at '),'AT');assert.equal(normalizeRegion('GB'),'GB');
  for(const value of [null,undefined,'','ZZ','US<script>','UK','EU','001'])assert.equal(normalizeRegion(value),'');
  for(const language of ['de','en']) {
    setLanguage(language);const options=regionOptions();
    assert.equal(options.length,249);assert.equal(new Set(options.map(item=>item.code)).size,249);
    assert.equal(options.find(item=>item.code==='AT').label,language==='de'?'Österreich (AT)':'Austria (AT)');
    const collator=new Intl.Collator(language==='de'?'de-AT':'en-GB');
    assert.ok(options.every((item,index)=>index===0||collator.compare(options[index-1].label,item.label)<=0));
    assert.ok(options.every(item=>/^[A-Z]{2}$/.test(item.code)));
  }
  setLanguage('de');
});
test('job region is normalized separately from the suggestion without inventing a missing value',()=>{
  for(const state of ['checking','ready','installing','failed','cancelled','complete']) {
    const job={id:'x',mode:'install',state,market:'de'};
    assert.equal(normalizeSetup({...base(),defaults:{market:'AT'},job}).job.market,'DE');
    assert.equal(normalizeSetup({...base(),defaults:{market:'AT'},job:{...job,market:undefined}}).job.market,'');
    assert.equal(normalizeSetup({...base(),job:{...job,market:'ZZ'}}).job.market,'');
  }
});
test('installation guidance follows machine phases, not localized error matching',()=>{
  const job={id:'x',mode:'install',state:'failed',failure_phase:'authentication',error:'Opaque localized error'};
  assert.match(installationHelp(normalizeSetup({...base(),job}).job),/Microsoft-Fenster/);
  assert.match(installationHelp({...job,failure_phase:'download'}),/Internetverbindung/);
  assert.match(installationHelp({...job,failure_phase:'paths'}),/Softwareverwaltung/);
  assert.equal(installationHelp({...job,mode:'existing'}),'');
  setLanguage('en');assert.match(installationHelp(job),/account that owns/);
  assert.match(installationHelp({...job,state:'installing',phase:'authentication'}),/opens separately/);setLanguage('de');
});

test('pause and resume need both genuine capabilities and matching download phase',()=>{
  const job={id:'x',mode:'install',state:'installing',phase:'download',can_pause:true};
  assert.deepEqual(downloadActions(normalizeSetup({...base(),job}).job),{pause:true,resume:false});
  for(const phase of ['authentication','bootstrap','pausing','failed'])assert.equal(downloadActions({...job,phase}).pause,false);
  for(const value of [false,undefined,'true',1])assert.equal(downloadActions(normalizeSetup({...base(),job:{...job,can_pause:value}}).job).pause,false);
  const paused={...job,phase:'paused',can_pause:false,can_resume:true};
  assert.deepEqual(downloadActions(paused),{pause:false,resume:true});assert.equal(setupBusy(paused),true);
  assert.deepEqual(downloadActions({...paused,state:'failed'}),{pause:false,resume:false});
  setLanguage('en');assert.match(installationHelp(paused),/Keep Flightdeck open/);setLanguage('de');
});
