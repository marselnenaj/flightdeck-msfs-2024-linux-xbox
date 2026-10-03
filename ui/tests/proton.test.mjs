import test from 'node:test';
import assert from 'node:assert/strict';
import {protonActions} from '../proton.js';
const status={runtime:{configured:true,path:'/one'},game:{state:'stopped'},setup:{busy:false}};
const data={runtime_path:'/one',can_restore:true,job:null};
test('Proton selection requires the current idle installation',()=>{
  assert.equal(protonActions(data,status).select,true);
  for(const flags of [{online:false},{reserved:true},{pending:true}])assert.equal(protonActions(data,status,flags).select,false);
  for(const state of ['starting','running','stopping','external'])assert.equal(protonActions(data,{...status,game:{state}}).select,false);
  assert.equal(protonActions({...data,runtime_path:'/two'},status).select,false);
  assert.equal(protonActions({...data,fenix:true},status).select,false);
  assert.equal(protonActions(data,{...status,setup:{busy:true}}).select,false);
  assert.equal(protonActions(null,null).select,false);
});
test('recovery remains possible after a failed sync or interrupted switch',()=>{
  const recovery={...data,error:'interrupted'};
  const attention={...status,cloud:{state:'attention'}};
  assert.equal(protonActions(recovery,attention).restore,true);
  assert.equal(protonActions(recovery,attention).select,false);
  for(const state of ['syncing','playing'])assert.equal(protonActions(recovery,{...status,cloud:{state}}).restore,false);
});
test('cancellation is bound to the running preparation',()=>{
  const job={state:'preparing',runtime_path:'/one'};
  assert.equal(protonActions({...data,job},status).cancel,true);
  assert.equal(protonActions({...data,job:{...job,runtime_path:'/two'}},status).cancel,false);
  assert.equal(protonActions({...data,job:{...job,state:'complete'}},status).cancel,false);
});
