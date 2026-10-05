import test from 'node:test';
import assert from 'node:assert/strict';
import {normalizeMods,modsMessage} from '../mods.js';
import {setLanguage} from '../i18n.js';

const base=()=>({state:'ready',message:'',folder_path:'/synthetic/Community',can_open:true,mods:[],count:0,limited:false});
test('only a known readable inventory can enable opening its actual folder',()=>{
  assert.equal(normalizeMods(base()).can_open,true);
  for(const state of ['unconfigured','unknown','missing','ambiguous','error'])assert.equal(normalizeMods({...base(),state}).can_open,false);
  for(const can_open of [false,undefined,'true',1])assert.equal(normalizeMods({...base(),can_open}).can_open,false);
  for(const folder_path of [null,'','relative/path'])assert.equal(normalizeMods({...base(),folder_path}).can_open,false);
});
test('missing or unknown inventory cannot become an empty successful list',()=>{
  for(const raw of [null,{}, {...base(),state:'future-state'}, {...base(),mods:null}, {...base(),mods:[null]}, {...base(),mods:[{id:'x',status:'future-state'}]}])assert.throws(()=>normalizeMods(raw));
  assert.equal(normalizeMods({...base(),count:undefined}).count,null);
  assert.equal(normalizeMods({...base(),count:undefined}).limited,true);
  assert.equal(normalizeMods({...base(),limited:undefined}).limited,true);
  assert.equal(normalizeMods({...base(),count:'0'}).count,null);
  assert.equal(normalizeMods({...base(),count:12}).limited,true);
});
test('manifest fields stay bounded literal text, not compatibility decisions',()=>{
  const item={id:'folder',name:'<script>'+ 'N'.repeat(2000),version:'<img>'+ 'V'.repeat(2000),creator:'A'.repeat(2000),status:'invalid_manifest',is_link:'true'};
  const data=normalizeMods({...base(),mods:[item],count:1});
  assert.equal(data.mods[0].name.length,240);assert.equal(data.mods[0].version.length,100);assert.equal(data.mods[0].creator.length,180);
  assert.equal(data.mods[0].is_link,false);assert.equal(data.mods[0].status,'invalid_manifest');assert.match(data.mods[0].name,/^<script>/);
  const large=normalizeMods({...base(),mods:Array(1001).fill({...item,status:'available'}),count:1001});assert.equal(large.mods.length,1000);assert.equal(large.limited,true);
});
test('unavailable reasons use localized help and preserve backend display text',()=>{
  setLanguage('de');assert.match(modsMessage(normalizeMods({...base(),state:'unconfigured'})),/Spielinstallation/);
  setLanguage('en');assert.match(modsMessage(normalizeMods({...base(),state:'missing'})),/missing/);
  assert.match(modsMessage(normalizeMods({...base(),state:'ambiguous'})),/Several/);
  assert.equal(modsMessage(normalizeMods({...base(),message:'Literal backend message'})),'Literal backend message');setLanguage('de');
});
