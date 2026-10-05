// Transient feedback, with enough reading time and no disappearing focused control.
export function createNotices(element, {schedule=setTimeout, clear=clearTimeout, now=()=>performance.now()}={}) {
  const message=element.querySelector('[data-notice-message]');
  const close=element.querySelector('button');
  let timer=null,remaining=0,started=0,hovered=false,focused=false;
  function stop() { if(timer!==null){clear(timer);timer=null;remaining=Math.max(0,remaining-(now()-started));} }
  function hide() {stop();element.hidden=true;message.textContent='';hovered=false;focused=false;}
  function resume() {
    if(element.hidden||hovered||focused||timer!==null)return;
    started=now();timer=schedule(()=>{timer=null;hide();},remaining);
  }
  element.addEventListener('mouseenter',()=>{hovered=true;stop();});
  element.addEventListener('mouseleave',()=>{hovered=false;resume();});
  element.addEventListener('focusin',()=>{focused=true;stop();});
  element.addEventListener('focusout',event=>{if(!element.contains(event.relatedTarget)){focused=false;resume();}});
  close.addEventListener('click',hide);
  return {show(value,error=false) {
    if(!value){hide();return;}
    stop();message.textContent=value; element.classList.toggle('error',error); element.hidden=!value;
    remaining=error?12000:6000;
    hovered=element.matches(':hover');focused=element.contains(element.ownerDocument.activeElement);
    resume();
  },hide};
}
