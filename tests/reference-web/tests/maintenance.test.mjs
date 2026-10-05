import test from 'node:test';
import assert from 'node:assert/strict';
import {maintenanceActions} from '../maintenance.js';
const status=()=>({csrf_token:'synthetic',runtime:{configured:true,path:'/one'},game:{state:'stopped'}});
const job={state:'ready',runtime_path:'/one'};
test('maintenance confirmation belongs to the reviewed runtime',()=>{
  assert.equal(maintenanceActions(job,status()).confirm,true);
  assert.equal(maintenanceActions(job,{...status(),runtime:{configured:true,path:'/two'}}).confirm,false);
  for(const state of ['checking','running','complete','failed','cancelled'])assert.equal(maintenanceActions({...job,state},status()).confirm,false);
});
test('game, sync, disconnection and other operations block maintenance',()=>{
  for(const state of ['starting','running','stopping','external'])assert.equal(maintenanceActions(job,{...status(),game:{state}}).preview,false);
  for(const flags of [{online:false},{reserved:true},{pending:true}])assert.equal(maintenanceActions(job,status(),flags).confirm,false);
  for(const state of ['syncing','playing'])assert.equal(maintenanceActions(job,{...status(),cloud:{state}}).confirm,false);
  assert.equal(maintenanceActions(job,{...status(),cloud:{state:'attention'}}).confirm,true);
  assert.equal(maintenanceActions(null,null).preview,false);
});
