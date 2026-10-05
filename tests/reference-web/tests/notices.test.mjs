import test from 'node:test';
import assert from 'node:assert/strict';
import {createNotices} from '../notices.js';

function fixture() {
  let time=0,serial=0;const timers=new Map(),listeners=new Map(),closeListeners=new Map();
  const message={textContent:''},close={addEventListener:(name,fn)=>closeListeners.set(name,fn)};
  const element={hidden:true,hovered:false,ownerDocument:{activeElement:null},
    classList:{toggle(){}},querySelector:selector=>selector==='button'?close:message,
    contains:target=>target===close,matches:()=>element.hovered,
    addEventListener:(name,fn)=>listeners.set(name,fn)};
  const notices=createNotices(element,{now:()=>time,clear:id=>timers.delete(id),schedule:(fn,delay)=>{timers.set(++serial,{at:time+delay,fn});return serial;}});
  const tick=ms=>{time+=ms;for(const [id,timer] of timers)if(timer.at<=time){timers.delete(id);timer.fn();}};
  const event=(name,relatedTarget=null)=>listeners.get(name)({relatedTarget});
  return {notices,element,message,close,tick,event,click:()=>closeListeners.get('click')(),timers};
}
test('normal notices disappear after six seconds; errors allow twelve seconds',()=>{
  const f=fixture();f.notices.show('Saved');f.tick(5999);assert.equal(f.element.hidden,false);
  f.tick(1);assert.equal(f.element.hidden,true);assert.equal(f.message.textContent,'');
  f.notices.show('Retry',true);f.tick(11999);assert.equal(f.element.hidden,false);f.tick(1);assert.equal(f.element.hidden,true);
});
test('replacement gets its own lifetime and the old timer cannot dismiss it',()=>{
  const f=fixture();f.notices.show('First');f.tick(5000);f.notices.show('Second');f.tick(1000);
  assert.equal(f.message.textContent,'Second');assert.equal(f.element.hidden,false);assert.equal(f.timers.size,1);
  f.tick(5000);assert.equal(f.element.hidden,true);
});
test('hover and keyboard focus pause independently and preserve remaining reading time',()=>{
  const f=fixture();f.notices.show('Read this');f.tick(2000);f.event('mouseenter');f.event('focusin');f.tick(20000);
  assert.equal(f.element.hidden,false);f.event('mouseleave');f.tick(10000);assert.equal(f.element.hidden,false);
  f.event('focusout');f.tick(3999);assert.equal(f.element.hidden,false);f.tick(1);assert.equal(f.element.hidden,true);
});
test('manual close clears timers and a later notice still expires after focus leaves',()=>{
  const f=fixture();f.notices.show('First');f.event('focusin');f.click();assert.equal(f.element.hidden,true);assert.equal(f.timers.size,0);
  f.notices.show('New notice');f.tick(6000);assert.equal(f.element.hidden,true);
  f.notices.show('Clear this');f.notices.show('');assert.equal(f.element.hidden,true);assert.equal(f.timers.size,0);
});

