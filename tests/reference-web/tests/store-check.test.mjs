import test from 'node:test';
import assert from 'node:assert/strict';
import {storeCheckActions} from '../store-check.js';
const status=()=>({csrf_token:'synthetic',runtime:{configured:true},game:{state:'stopped'},setup:{busy:false}});
test('Store check requires an idle runtime and a local session',()=>{
  assert.equal(storeCheckActions(null,status()).start,true);
  assert.equal(storeCheckActions(null,null).start,false);
  for(const state of ['starting','running','external','stopping'])assert.equal(storeCheckActions(null,{...status(),game:{state}}).start,false);
  for(const flags of [{online:false},{pending:true},{reserved:true}])assert.equal(storeCheckActions(null,status(),flags).start,false);
  assert.equal(storeCheckActions(null,{...status(),setup:{busy:true}}).start,false);
  assert.equal(storeCheckActions(null,{...status(),cloud:{state:'syncing'}}).start,false);
});
test('Store check can be cancelled during its own reservation',()=>{
  const job={state:'running'};
  assert.equal(storeCheckActions(job,status(),{reserved:true}).cancel,true);
  assert.equal(storeCheckActions(job,status()).start,false);
  assert.equal(storeCheckActions(job,status(),{pending:true}).cancel,false);
  assert.equal(storeCheckActions({state:'passed'},status()).cancel,false);
});
