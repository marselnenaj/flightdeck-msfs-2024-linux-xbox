import test from 'node:test';
import assert from 'node:assert/strict';
import {normalizeCloud,cloudActions,cloudTitle} from '../cloud-saves.js';
import {setLanguage} from '../i18n.js';
const raw=()=>({available:true,mode:'download_and_import',sync_supported:false,can_check:true,can_download:true,can_cancel:false,can_prepare_import:true,can_import:false,plan:null,job:null});
const preview=()=>({id:'opaque-preview',container_count:2,blob_count:3,total_bytes:100,local_exists:true,local_container_count:2,add_count:1,replace_count:1,delete_count:1,unchanged_count:0,conflict_count:2});
const status=()=>({runtime:{configured:true},game:{state:'stopped'},csrf_token:'synthetic'});
const options={online:true,fresh:true};
const job=(state='succeeded')=>({id:'synthetic-job',operation:'check',state,message:'<script>literal</script>',result:{container_count:2,blob_count:null,total_bytes:100,downloaded:false,rechecked:false}});
test('missing API or a different protocol is never an empty inventory',()=>{
 for(const value of [null,{}, {...raw(),mode:'sync'}, {...raw(),sync_supported:true},{...raw(),job:undefined}])assert.throws(()=>normalizeCloud(value));
 assert.equal(normalizeCloud(raw()).job,null);
 assert.equal(normalizeCloud({...raw(),available:false}).available,false);
});
test('successful result requires typed, bounded, operation-specific evidence',()=>{
 for(const value of [{container_count:-1},{container_count:'0'},{total_bytes:Infinity},{blob_count:true},{downloaded:true}])assert.throws(()=>normalizeCloud({...raw(),job:{...job(),result:{...job().result,...value}}}));
 const copy={...job(),operation:'download',result:{...job().result,blob_count:3,downloaded:true,rechecked:true}};
 assert.equal(normalizeCloud({...raw(),job:copy}).job.result.blob_count,3);
 assert.throws(()=>normalizeCloud({...raw(),job:{...copy,result:{...copy.result,rechecked:false}}}));
 assert.throws(()=>normalizeCloud({...raw(),job:{...job(),id:'x'.repeat(129)}}));
});
test('check and download need fresh online idle state and exact capability',()=>{
 const d=normalizeCloud(raw());assert.equal(cloudActions(d,status(),options).check,true);
 for(const delta of [{online:false},{fresh:false},{pending:true},{reserved:true}])assert.equal(cloudActions(d,status(),{...options,...delta}).download,false);
 for(const state of ['running','starting','stopping','external','unknown'])assert.equal(cloudActions(d,{...status(),game:{state}},options).check,false);
 assert.equal(cloudActions({...d,can_download:false},status(),options).download,false);
 assert.equal(cloudActions(d,{...status(),csrf_token:''},options).check,false);
});
test('cancel is tied to a real running job, including when another reservation exists',()=>{
 const d=normalizeCloud({...raw(),can_cancel:true,job:job('running')});
 assert.equal(cloudActions(d,status(),options).check,false);
 assert.equal(cloudActions(d,status(),{...options,reserved:true}).cancel,true);
 assert.equal(cloudActions({...d,can_cancel:false},status(),options).cancel,false);
 assert.equal(cloudActions({...d,job:job('cancelled')},status(),options).cancel,false);
});
test('failed jobs discard any misleading successful result',()=>{
 const d=normalizeCloud({...raw(),job:job('failed')});assert.equal(d.job.result,null);
 setLanguage('en');assert.equal(cloudTitle(d),'Cloud request failed');
 assert.equal(cloudTitle(normalizeCloud(raw())),'Not checked yet');
 assert.equal(cloudTitle(normalizeCloud({...raw(),job:{...job(),result:{...job().result,container_count:0}}})),'No cloud saves found');
 setLanguage('de');assert.equal(cloudTitle(d),'Cloud-Anfrage fehlgeschlagen');
});
test('an import needs a validated preview and exact server capability',()=>{
 const p=preview(),d=normalizeCloud({...raw(),plan:p,can_import:true});
 assert.equal(cloudActions(d,status(),options).import,true);
 assert.equal(cloudActions({...d,plan:null},status(),options).import,false);
 assert.equal(cloudActions({...d,can_import:false},status(),options).import,false);
 assert.equal(cloudActions(d,status(),{...options,fresh:false}).import,false);
 assert.equal(cloudTitle(d),'Cloud und lokaler Spielstand');
 for(const change of [{id:''},{id:'a'.repeat(129)},{local_exists:'yes'},{delete_count:-1},{conflict_count:Infinity}])assert.throws(()=>normalizeCloud({...raw(),plan:{...p,...change}}));
 assert.throws(()=>normalizeCloud({...raw(),mode:'download_only',plan:p}));
});
test('successful import and preview require operation-specific evidence',()=>{
 const r={...job().result,blob_count:3,downloaded:true,rechecked:true,imported:true,durability_confirmed:true};
 const value={...raw(),job:{...job(),operation:'import',result:r}};
 assert.equal(normalizeCloud(value).job.result.imported,true);
 assert.equal(cloudTitle(normalizeCloud(value)),'Cloud-Spielstände übernommen');
 assert.throws(()=>normalizeCloud({...value,job:{...value.job,result:{...r,durability_confirmed:undefined}}}));
 assert.throws(()=>normalizeCloud({...value,job:{...value.job,operation:'prepare-import'}}));
 assert.equal(normalizeCloud({...value,job:{...value.job,operation:'prepare-import',result:{...r,prepared_for_import:true}}}).job.result.prepared_for_import,true);
});
test('upload requires native sync capability and explicit reviewed comparison',()=>{
 const r={...raw(),mode:'manual_sync',sync_supported:true,can_upload:true,plan:preview()};
 assert.equal(cloudActions(normalizeCloud(r),status(),options).upload,true);
 assert.equal(cloudActions(normalizeCloud({...r,plan:null}),status(),options).upload,false);
 assert.equal(cloudActions(normalizeCloud({...r,mode:'download_and_import',sync_supported:false}),status(),options).upload,false);
 assert.throws(()=>normalizeCloud({...r,sync_supported:false}));
});
test('verified upload needs full result evidence and partial failure stays visible',()=>{
 const result={container_count:2,blob_count:3,total_bytes:100,downloaded:false,rechecked:true,uploaded:true,changed_containers:1,lease_released:true,baseline_saved:true};
 const data={...raw(),mode:'manual_sync',sync_supported:true,job:{...job(),operation:'upload',result}};
 assert.equal(cloudTitle(normalizeCloud(data)),'Xbox-Cloud aktualisiert');
 for(const delta of [{uploaded:false},{rechecked:false},{changed_containers:-1},{baseline_saved:null},{lease_released:undefined}])assert.throws(()=>normalizeCloud({...data,job:{...data.job,result:{...result,...delta}}}));
 const partial=normalizeCloud({...data,job:{...data.job,state:'failed',recovery_required:true}});
 assert.equal(partial.job.result,null);
 assert.equal(cloudTitle(partial),'Upload nicht vollständig bestätigt');
});
test('restore is bound to a specific backup and fresh capability',()=>{
 const d=normalizeCloud({...raw(),restore_id:'opaque-backup',can_restore:true});
 assert.equal(cloudActions(d,status(),options).restore,true);
 assert.equal(cloudActions({...d,restore_id:null},status(),options).restore,false);
 assert.equal(cloudActions(d,status(),{...options,fresh:false}).restore,false);
 assert.throws(()=>normalizeCloud({...raw(),restore_id:42}));
});

test('automatic mode retains manual capabilities behind separate controls',()=>{
 const d=normalizeCloud({...raw(),mode:'automatic_sync',sync_supported:true,automatic_sync:true,can_upload:true,plan:preview()});
 assert.equal(cloudActions(d,status(),options).upload,true);
 for(const state of ['syncing','playing','attention'])assert.equal(cloudActions(d,{...status(),cloud:{state}},options).upload,false);
});
