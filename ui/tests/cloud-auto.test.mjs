import test from 'node:test';
import assert from 'node:assert/strict';
import {normalizeAutomatic,normalizeStatus,actionPermissions,gamePresentation} from '../state.js';
import {automaticActions,automaticTitle} from '../cloud-saves.js';
import {setLanguage} from '../i18n.js';
const cloud=()=>({enabled:true,state:'idle',phase:null,message:'Synthetic',error_code:null,can_retry:false,can_play_local:false,can_cancel:false,request_id:null,last_synced_at:null,conflict:false,summary:null});
const raw=(automatic=cloud())=>({runtime:{configured:true,ready:true},game:{state:'stopped',can_start:true},saves:{available:true,mode:'local',can_backup:true},cloud:automatic,csrf_token:'synthetic'});
const id='123456781234423482341234567890ab';
test('unknown automatic fields never create active actions or successful sync',()=>{
 for(const delta of [{state:'success'},{enabled:'true'},{phase:'other'},{request_id:'private path'}])assert.equal(normalizeAutomatic({...cloud(),...delta}),null);
 assert.equal(normalizeAutomatic(undefined),null);
 assert.equal(normalizeAutomatic({...cloud(),message:'a'.repeat(1800),summary:{container_count:-1,local_container_count:2,private:'secret'}}).message.length,1600);
 assert.deepEqual(normalizeAutomatic({...cloud(),summary:{container_count:0,local_container_count:2,private:'secret'}}).summary,{container_count:0,local_container_count:2});
});
test('sync reserves launch/configuration/backups but managed playing retains stop',()=>{
 const syncing=normalizeStatus(raw({...cloud(),state:'syncing',phase:'before_start',request_id:id}));
 const p=actionPermissions(syncing,true,null);
 assert.equal(p.start,false);assert.equal(p.configure,false);assert.equal(p.backup,false);
 const r=raw({...cloud(),state:'playing',phase:'before_start',request_id:id});r.game={state:'running',managed:true,can_stop:true};
 assert.equal(actionPermissions(normalizeStatus(r),true,null).stop,true);
 assert.equal(actionPermissions(normalizeStatus(raw({...cloud(),state:'local'})),true,null).start,true);
});
test('retry/local play/cancel/resolve require exact fresh capability and request',()=>{
 const status=normalizeStatus(raw({...cloud(),state:'attention',phase:'before_start',request_id:id,can_retry:true,can_play_local:true}));
 const opts={online:true};assert.equal(automaticActions(status,opts).retry,true);assert.equal(automaticActions(status,opts)['play-local'],true);
 for(const delta of [{online:false},{pending:true},{reserved:true}])assert.equal(automaticActions(status,{...opts,...delta}).retry,false);
 for(const field of ['request_id','enabled','can_retry'])assert.equal(automaticActions({...status,cloud:{...status.cloud,[field]:null}},opts).retry,false);
 const conflict={...status,cloud:{...status.cloud,conflict:true}};
 assert.equal(automaticActions(conflict,opts).cloud,true);assert.equal(automaticActions(conflict,opts).local,true);assert.equal(automaticActions(conflict,opts)['play-local'],false);
 const after={...status,cloud:{...status.cloud,phase:'after_exit',can_play_local:false}};assert.equal(automaticActions(after,opts)['play-local'],false);
 assert.equal(automaticActions({...after,cloud:{...after.cloud,can_play_local:true}},opts)['play-local'],true);
 assert.equal(automaticActions({...status,cloud:{...status.cloud,state:'syncing',can_cancel:true}},opts)['cancel-auto'],true);
 assert.equal(automaticActions({...status,game:{state:'external'}},opts).retry,false);
});
test('pre/post sync and disabled capability have honest localized status',()=>{
 setLanguage('en');assert.equal(automaticTitle({...cloud(),enabled:false}),'Automatic cloud sync unavailable');
 assert.equal(gamePresentation(normalizeStatus(raw({...cloud(),state:'syncing',phase:'before_start'}))).action,'Preparing to launch …');
 assert.equal(gamePresentation(normalizeStatus(raw({...cloud(),state:'syncing',phase:'after_exit'}))).action,'Saving progress …');
 setLanguage('de');
});
