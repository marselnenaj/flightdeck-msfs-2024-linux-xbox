import test from 'node:test';
import assert from 'node:assert/strict';
import {normalizeTransfer, jobProgress} from '../state.js';
import {normalizeSetup} from '../setup.js';
import {normalizeUpdate} from '../updates.js';
import {setLanguage} from '../i18n.js';

const transfer = (extra={}) => ({kind:'game',received_bytes:2_500_000_000,verified_bytes:2_000_000_000,total_bytes:10_000_000_000,completed_files:2,total_files:10,...extra});
const job = (extra={}) => ({id:'fixture',mode:'install',state:'installing',phase:'download',progress:100,transfer:transfer(),...extra});

test('transfer rejects malformed or inconsistent counts instead of coercing them',()=>{
  for(const raw of [null,{},[],{...transfer(),kind:'other'}])assert.equal(normalizeTransfer(raw),null);
  for(const key of ['received_bytes','verified_bytes','total_bytes','completed_files','total_files']) {
    for(const value of [-1,NaN,Infinity,1.5,'5',true,Number.MAX_SAFE_INTEGER+1])assert.equal(normalizeTransfer(transfer({[key]:value})),null,`${key}: ${value}`);
  }
  for(const delta of [{received_bytes:null},{verified_bytes:undefined},{total_bytes:0},{total_files:0},{verified_bytes:2_500_000_001},{received_bytes:10_000_000_001},{completed_files:11}])assert.equal(normalizeTransfer(transfer(delta)),null);
  assert.ok(normalizeTransfer(transfer({received_bytes:0,verified_bytes:0,total_bytes:null,completed_files:null,total_files:null})));
});

test('setup, updates and repair preserve the same normalized transfer',()=>{
  const source=job();
  assert.deepEqual(normalizeSetup({available:true,job:source}).job.transfer,transfer());
  for(const operation of ['update','repair'])assert.deepEqual(normalizeUpdate({available:true,job:{...source,mode:'update',operation}}).job.transfer,transfer());
});

test('byte percentages supersede phase percentages and never round unfinished verification to 100',()=>{
  assert.equal(jobProgress(job()).value,25);
  for(const delta of [{received_bytes:10_000_000_000},{received_bytes:9_999_999_999,verified_bytes:9_999_999_999}])assert.equal(jobProgress(job({transfer:transfer(delta)})).value,99.9);
  assert.equal(jobProgress(job({transfer:transfer({received_bytes:10_000_000_000,verified_bytes:10_000_000_000})})).value,99.9);
  assert.equal(jobProgress(job({transfer:transfer({received_bytes:10_000_000_000,verified_bytes:10_000_000_000,completed_files:10})})).value,100);
  assert.equal(jobProgress(job({phase:'bootstrap',transfer:transfer({kind:'components',received_bytes:10_000_000_000,verified_bytes:10_000_000_000,completed_files:null,total_files:null})})).value,100);
  const large=Number.MAX_SAFE_INTEGER;
  assert.equal(jobProgress(job({transfer:transfer({received_bytes:large-1,verified_bytes:large-1,total_bytes:large})})).value,99.9);
});

test('unknown or rejected totals stay indeterminate and do not manufacture byte counts',()=>{
  setLanguage('en');
  assert.deepEqual(jobProgress(job({transfer:transfer({total_bytes:null})})),{visible:true,value:null,detail:'2.5 GB received'});
  for(const raw of [null,transfer({total_bytes:-1}),transfer({verified_bytes:3_000_000_000})])assert.deepEqual(jobProgress(job({transfer:raw})),{visible:true,value:null,detail:''});
  setLanguage('de');
});

test('DE/EN quantities use decimal units and pause preserves the displayed evidence',()=>{
  setLanguage('de');assert.equal(jobProgress(job()).detail,'2,5 GB von 10 GB · 25 %');
  assert.deepEqual(jobProgress(job({phase:'paused'})),{visible:true,value:25,detail:'Pausiert · 2,5 GB von 10 GB · 25 %'});
  setLanguage('en');assert.equal(jobProgress(job()).detail,'2.5 GB of 10 GB · 25 %');
  assert.deepEqual(jobProgress(job({phase:'paused',transfer:transfer({total_bytes:null})})),{visible:false,value:null,detail:'Paused · 2.5 GB received'});
  assert.equal(jobProgress(job({transfer:transfer({received_bytes:512_000_000,verified_bytes:0})})).detail,'512 MB of 10 GB · 5.1 %');
  setLanguage('de');
});

test('transfer details stop at phase boundaries and component/game bytes never cross phases',()=>{
  const components=transfer({kind:'components'});
  assert.equal(jobProgress(job({phase:'bootstrap',transfer:components})).value,25);
  assert.equal(jobProgress(job({phase:'bootstrap'})).detail,'');
  assert.equal(jobProgress(job({transfer:components})).detail,'');
  for(const phase of ['authentication','provision','verify_update','switch_update']) {
    const result=jobProgress(job({phase,progress:null}));
    assert.equal(result.detail,'');assert.equal(result.value,null);
  }
  for(const state of ['ready','complete','failed','cancelled']) {
    const result=jobProgress(job({state}));assert.equal(result.visible,false);assert.equal(result.detail,'');
  }
});
