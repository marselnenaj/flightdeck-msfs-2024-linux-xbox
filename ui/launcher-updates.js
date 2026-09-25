import {t} from './i18n.js';
import {stringValue,formatBytes,formatDate} from './state.js';

export function normalizeLauncherUpdate(raw) {
  if(!raw||typeof raw.managed!=='boolean'||typeof raw.installed_version!=='string')throw new Error(t('Launcher-Updatestatus konnte nicht geladen werden.'));
  const job=raw.job&&typeof raw.job.id==='string'?{
    id:raw.job.id,operation:stringValue(raw.job.operation),state:stringValue(raw.job.state),phase:stringValue(raw.job.phase),
    message:stringValue(raw.job.message,'',1500),error:stringValue(raw.job.error,'',1500),can_cancel:raw.job.can_cancel===true,
    progress:Number.isFinite(raw.job.progress)?Math.min(100,Math.max(0,raw.job.progress)):null,
    received:Number.isSafeInteger(raw.job.received)&&raw.job.received>=0?raw.job.received:0,
    total:Number.isSafeInteger(raw.job.total)&&raw.job.total>0?raw.job.total:null,
  }:null;
  return {...Object.fromEntries(['managed','pending_restart','can_check','can_install','can_rollback','can_restart','busy'].map(k=>[k,raw[k]===true])),
    installed_version:stringValue(raw.installed_version,'',50),latest_version:stringValue(raw.latest_version,'',50),
    check_id:stringValue(raw.check_id),checked_at:stringValue(raw.checked_at),notes:stringValue(raw.notes,'',12000),
    unavailable_reason:stringValue(raw.unavailable_reason,'',1500),
    update_available:typeof raw.update_available==='boolean'?raw.update_available:null,job};
}

export function launcherUpdateTitle(data) {
  if(!data)return t('Updatestatus wird geladen …');
  if(data.job?.state==='running')return t(({checking:'Suche nach Flightdeck-Updates …',downloading:'Flightdeck wird heruntergeladen …',verifying:'Download wird geprüft …',installing:'Flightdeck wird installiert …',restart:'Flightdeck wird neu geöffnet …'})[data.job.phase]||'Update wird vorbereitet …');
  if(data.pending_restart)return t('Bereit zum Neustart');
  if(data.job?.state==='failed')return t('Update nicht abgeschlossen');
  if(data.update_available===true)return t('Flightdeck {version} ist verfügbar',{version:data.latest_version});
  if(data.update_available===false)return t('Flightdeck ist aktuell');
  return t('Noch nicht nach Updates gesucht');
}

export function createLauncherUpdates({request,getStatus,isOnline,isReserved=()=>false,refreshStatus,changed,notice,availableChanged=()=>{}}) {
  const $=id=>document.getElementById('launcher-update-'+id);
  let data=null,loading=null,pending=false,fresh=false,error='',actionError='',confirmRollback=false,restarting=false,restartStarted=0,reserved=false;
  function render() {
    if(restarting&&fresh&&!data?.pending_restart){location.reload();return;}
    if(restarting&&(data?.job?.operation==='restart'&&data.job.state==='failed'||Date.now()-restartStarted>90000)){
      restarting=false;actionError=data?.job?.error||t('Der Neustart dauert länger als erwartet. Öffne Flightdeck erneut über das Anwendungsmenü.');
    }
    const active=data?.job?.state==='running',allowed=isOnline()&&fresh&&!pending&&!restarting;
    availableChanged(fresh&&data?.update_available===true&&!data.pending_restart);
    $('title').textContent=launcherUpdateTitle(data);
    $('installed').textContent=data?.installed_version||getStatus()?.app.version||'—';
    $('latest').textContent=data?.latest_version||t('Noch nicht geprüft');
    $('message').textContent=data?.job?.message||t('Lade neue Flightdeck-Versionen direkt von GitHub. Deine Einstellungen bleiben erhalten.');
    $('message').hidden=!!data?.job?.error&&data.job.message===data.job.error;
    $('error').textContent=error||actionError||data?.job?.error||'';$('error').hidden=!$('error').textContent;
    $('check').disabled=!allowed||!data?.can_check;
    $('check').hidden=active||data?.pending_restart===true;
    $('check').textContent=t(data?.job?.state==='failed'?'Erneut versuchen':'Nach Updates suchen');
    $('check').classList.toggle('dark',!data?.update_available);$('check').classList.toggle('secondary',!!data?.update_available);
    $('install').hidden=data?.update_available!==true||active||data?.pending_restart===true;
    $('install').disabled=!allowed||isReserved()||!data?.can_install;
    $('restart').hidden=!data?.pending_restart;$('restart').disabled=!allowed||isReserved()||!data?.can_restart;
    $('cancel').hidden=!active;$('cancel').disabled=!allowed||!data?.job?.can_cancel;
    $('rollback').disabled=!allowed||isReserved()||!data?.can_rollback;
    $('rollback-area').hidden=!data?.can_rollback;
    if(!data?.can_rollback)confirmRollback=false;
    $('rollback-confirm').hidden=!confirmRollback;
    $('rollback-yes').disabled=!allowed||isReserved()||!data?.can_rollback;
    $('busy').hidden=!(data?.busy||isReserved())||active;
    $('unmanaged').textContent=data?.unavailable_reason||'';$('unmanaged').hidden=!data?.unavailable_reason;
    $('checked').textContent=data?.checked_at?t('Zuletzt geprüft: {time}',{time:formatDate(data.checked_at)||'—'}):'';
    $('notes').textContent=data?.notes||'';$('details').hidden=!data?.notes;
    const progress=$('progress');progress.hidden=!active;
    if(data?.job?.progress===null)progress.removeAttribute('value');else progress.value=data?.job?.progress??0;
    $('transfer').hidden=!active||!data?.job?.total;
    $('transfer').textContent=data?.job?.total?t('{received} von {total}',{received:formatBytes(data.job.received),total:formatBytes(data.job.total)}):'';
    $('card').setAttribute('aria-busy',String(active||pending));
    const next=!!(pending||restarting||active&&data.job.operation!=='check');
    if(next!==reserved){reserved=next;changed(next);}
  }
  async function load() {
    if(loading)return loading;
    loading=(async()=>{
      try {data=normalizeLauncherUpdate(await request('/api/launcher-update'));fresh=true;error='';}
      catch(failure){fresh=false;error=failure.message;}
      finally{loading=null;render();}
    })();return loading;
  }
  async function action(operation) {
    const key=operation==='cancel'?'can_cancel':operation==='install'?'can_install':operation==='rollback'?'can_rollback':operation==='restart'?'can_restart':'can_check';
    const permitted=()=>fresh&&isOnline()&&!restarting&&(!['install','rollback','restart'].includes(operation)||!isReserved())&&(operation==='cancel'?data?.job?.[key]:data?.[key]);
    if(pending||!permitted())return;
    pending=true;error='';actionError='';confirmRollback=false;render();
    try {
      if(loading)await loading;
      if(!permitted())return;
      const result=await request('/api/launcher-update/'+operation,{method:'POST',token:getStatus().csrf_token,
        body:operation==='install'?{check_id:data.check_id}:operation==='cancel'?{job_id:data.job.id}:{}});
      if(operation==='restart'){data.job=result.job||null;restarting=true;restartStarted=Date.now();notice(t('Flightdeck wird neu geöffnet …'));}
    } catch(failure){actionError=failure.message;notice(actionError,true);}
    finally {pending=false;if(!restarting){await load();await refreshStatus();}render();}
  }
  for(const actionName of ['check','install','cancel','restart'])$(actionName).addEventListener('click',()=>void action(actionName));
  $('rollback').addEventListener('click',()=>{confirmRollback=true;render();});
  $('rollback-no').addEventListener('click',()=>{confirmRollback=false;render();});
  $('rollback-yes').addEventListener('click',()=>{if(confirmRollback)void action('rollback');});
  setInterval(()=>{if(!document.hidden&&!pending&&(location.hash==='#updates'||data?.job?.state==='running'||restarting))void load();},1500);
  return {load,render};
}
