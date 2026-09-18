import {t} from './i18n.js';
import {stringValue,formatCount,renderJobProgress} from './state.js';
import {normalizeSetup, setupBusy, downloadActions} from './setup.js';

export function normalizeIntegrity(raw) {
  const source=raw?.result,keys=['checked','missing','changed','unreadable','total'];
  const valid=source&&typeof source.healthy==='boolean'&&keys.every(k=>Number.isSafeInteger(source[k])&&source[k]>=0);
  return {available:raw?.available===true,unavailable_reason:stringValue(raw?.unavailable_reason,'',1200),can_check:raw?.can_check===true,
    result:valid?Object.fromEntries([...keys.map(k=>[k,source[k]]),['healthy',source.healthy]]):null};
}

export function normalizeUpdate(raw) {
  if (!raw || typeof raw.available !== 'boolean') throw new Error(t('Updatestatus nicht verfügbar.'));
  const version=value=>typeof value==='string'&&value.trim()?value.slice(0,128):null;
  const job=raw.job?.mode==='update'?normalizeSetup({available:true,job:raw.job}).job:null;
  return {available:raw.available,unavailable_reason:stringValue(raw.unavailable_reason,'',1200),
    installed_version:version(raw.installed_version),latest_version:version(raw.latest_version),
    update_available:typeof raw.update_available==='boolean'?raw.update_available:null,
    can_check:raw.can_check===true,can_start:raw.can_start===true,can_rollback:raw.can_rollback===true,
    auth_required:raw.auth_required===true,integrity:normalizeIntegrity(raw.integrity),can_repair:raw.can_repair===true,job};
}

export function updateActions(data,status,{online=false,fresh=false,pending=false,reserved=false,setupJob=null}={}) {
  const job=data?.job;
  const otherJob=reserved&&(!job||setupJob?.id!==job.id);
  const enabled=!!data&&online&&fresh&&!pending&&!otherJob&&status?.runtime.configured===true&&status?.game.state==='stopped'&&!!status?.csrf_token;
  const idle=!setupBusy(job),download=downloadActions(job);
  return {check:enabled&&data.available&&idle&&data.can_check&&!data.auth_required,
    signIn:enabled&&idle&&data.auth_required&&(job?.operation==='repair'?data.can_repair:data.available&&data.can_check),
    verify:enabled&&idle&&data.integrity?.available&&data.integrity.can_check,
    repair:enabled&&idle&&data.can_repair&&!data.auth_required,
    start:enabled&&data.available&&job?.state==='ready'&&data.can_start&&(data.update_available===true||job?.operation==='repair')&&!!data.latest_version&&!!data.installed_version,
    pause:enabled&&download.pause,resume:enabled&&download.resume,cancel:enabled&&(setupBusy(job)||job?.state==='ready'),
    rollback:enabled&&idle&&data.can_rollback};
}

export function updateTitle(data) {
  if(!data)return t('Updatestatus wird geladen …');
  const job=data.job;
  if(job?.operation==='verify'&&setupBusy(job))return t('Spieldateien werden geprüft');
  if(job?.operation==='repair'&&job.state==='ready')return t('Reparatur vorbereitet');
  if(job?.operation==='repair'&&job.state==='complete')return t('Reparatur abgeschlossen');
  if(job?.operation==='verify'&&job.state==='complete')return t(data.integrity.result?.healthy===true?'Spieldateien geprüft':'Dateiprüfung abgeschlossen');
  if(!data.available)return t('Updates derzeit nicht verfügbar');
  if(job?.state==='checking')return t('MSFS-Version wird geprüft');
  if(job?.state==='installing')return t(({authentication:'Microsoft-Anmeldung',download:'Update wird heruntergeladen',pausing:'Download wird pausiert',paused:'Download pausiert',verify_update:'Update wird geprüft',switch_update:'Spielversion wird gewechselt'})[job.phase]||'Update wird vorbereitet');
  if(data.auth_required)return t('Anmeldung zum Prüfen erforderlich');
  if(job?.state==='failed')return t('Update nicht abgeschlossen');
  if(job?.state==='cancelled')return t('Update abgebrochen');
  if(data.update_available===true&&data.latest_version)return t('Eine neue Spielversion ist verfügbar');
  if(data.update_available===false&&data.latest_version)return t('Deine Spielversion ist aktuell');
  return t('Noch nicht nach Updates gesucht');
}

export function createUpdates({request,getStatus,isOnline,getSetupJob,isReserved,refreshStatus,refreshSetup,renderChecks,changed}) {
  const $=id=>document.getElementById(id);
  let data=null,loading=null,foregroundLoading=false,pending=false,fresh=false,error='',actionError='',loadedRuntime,confirmRollback=false;
  let reservation=false;
  const current=()=>fresh&&loadedRuntime===getStatus()?.runtime.path;
  const actions=(actionPending=pending)=>updateActions(data,getStatus(),{online:isOnline(),fresh:current(),pending:actionPending,reserved:isReserved(),setupJob:getSetupJob()});
  function reserve() {
    const next=pending||setupBusy(data?.job);
    if(next!==reservation){reservation=next;changed(next);}
  }
  function render() {
    const job=data?.job,allowed=actions();
    $('update-title').textContent=updateTitle(data);
    $('update-installed').textContent=data?.installed_version||t('Nicht bekannt');
    $('update-latest').textContent=data?.latest_version||t('Noch nicht geprüft');
    $('update-message').textContent=job?.message||(data?.available===false?data.unavailable_reason||t('Für diese Installation ist die Update-Funktion noch nicht verfügbar.')
      :t('Die Prüfung lädt kein Update herunter. Du startest den Download anschließend selbst.'));
    const failure=actionError||error||job?.error||'';
    $('update-error').textContent=failure;$('update-error').hidden=!failure;
    $('update-message').hidden=!!failure&&$('update-message').textContent===failure;
    $('update-stale').hidden=!data||current()||foregroundLoading;
    const gameBusy=getStatus()?.game.state!=='stopped';
    const otherJob=isReserved()&&(!job||getSetupJob()?.id!==job.id);
    $('update-busy').hidden=!gameBusy&&!otherJob;
    $('update-busy').textContent=t(gameBusy?'Beende den Simulator, bevor du nach Updates suchst oder die Spielversion wechselst.':'Schließe zuerst die laufende Einrichtung ab.');
    $('update-setup').hidden=getStatus()?.runtime.configured!==false;
    $('update-check').hidden=setupBusy(job)||data?.auth_required===true;
    $('update-check').classList.toggle('dark',job?.state!=='ready');
    $('update-check').classList.toggle('secondary',job?.state==='ready');
    $('update-sign-in').hidden=!data?.auth_required||setupBusy(job);
    $('update-start').hidden=job?.state!=='ready'||(data?.update_available!==true&&job?.operation!=='repair');
    $('update-start-label').textContent=t(job?.operation==='repair'?'Spiel neu herunterladen':'Update herunterladen');
    $('update-pause').hidden=!downloadActions(job).pause&&job?.phase!=='pausing';
    $('update-pause').textContent=t(job?.phase==='pausing'?'Wird pausiert …':'Download pausieren');
    $('update-resume').hidden=!downloadActions(job).resume;
    $('update-cancel').hidden=!setupBusy(job)&&job?.state!=='ready';
    $('update-cancel').textContent=t(job?.state==='ready'?'Später aktualisieren':'Abbrechen');
    for(const [id,action] of [['check','check'],['sign-in','signIn'],['start','start'],['pause','pause'],['resume','resume'],['cancel','cancel'],['rollback','rollback'],['rollback-yes','rollback'],['verify','verify'],['repair-check','repair']])$('update-'+id).disabled=!allowed[action]||foregroundLoading;
    $('update-refresh').disabled=foregroundLoading||pending;
    $('update-refresh').textContent=t(foregroundLoading?'Status wird geladen …':'Status neu laden');
    const integrity=data?.integrity,result=integrity?.result;
    $('integrity-status').textContent=integrity?.available===false?integrity.unavailable_reason||t('Für diese Installation fehlt ein vollständiger Prüfindex.')
      :result?t(result.healthy?'Keine Abweichungen gefunden.':'Die Dateiprüfung hat Abweichungen gefunden.'):t('Noch keine Dateiprüfung durchgeführt.');
    $('integrity-results').hidden=!result;
    for(const key of ['checked','missing','changed','unreadable','total'])$('integrity-'+key).textContent=result?formatCount(result[key]):'—';
    $('integrity-verify-help').hidden=integrity?.available!==true;
    $('integrity-repair-help').textContent=t('Eine Reparatur lädt das vollständige Basisspiel neu. Dabei kann die aktuell verfügbare neuere Storeversion installiert werden. Spielstände und Add-ons bleiben getrennt erhalten.');
    renderJobProgress($('update-progress'), $('update-transfer'), job);
    let help='';
    if(job?.state==='installing'&&job.phase==='authentication')help=t('Das Microsoft-Anmeldefenster öffnet sich separat. Wähle dort das Konto, mit dem du die Xbox-PC-Version gekauft hast.');
    else if(job?.phase==='paused'&&job.state==='installing')help=t('Der Download ist pausiert. Lass Flightdeck geöffnet, um ihn später fortzusetzen. Fertige, geprüfte Dateien bleiben erhalten; die laufende Datei beginnt erneut.');
    else if(downloadActions(job).pause)help=t('Du kannst den Download pausieren und in dieser geöffneten Flightdeck-Sitzung fortsetzen. Fertige, geprüfte Dateien bleiben erhalten; die laufende Datei beginnt erneut.');
    $('update-help').textContent=help;$('update-help').hidden=!help;
    $('update-details').hidden=!job?.checks.length;
    renderChecks('update-checks',job?.checks||[]);
    $('update-rollback-area').hidden=!data?.can_rollback;
    if(!allowed.rollback)confirmRollback=false;
    $('update-rollback-confirm').hidden=!confirmRollback;
  }
  async function load({background=false}={}) {
    if(loading)return loading;
    const runtime=getStatus()?.runtime.path;
    foregroundLoading=!background;
    loading=(async()=>{
      try{data=normalizeUpdate(await request('/api/game-update'));loadedRuntime=runtime;fresh=true;error='';}
      catch(failure){fresh=false;error=failure.message;}
      finally{loading=null;foregroundLoading=false;reserve();render();}
    })();
    render();return loading;
  }
  async function mutate(action,path,body={}) {
    if(!actions()[action])return;
    pending=true;error='';actionError='';confirmRollback=false;reserve();render();
    try{
      // Preserve click intent during a quiet poll, then recheck its fresh result
      // before posting. Never let that earlier read overwrite the mutation.
      if(loading)await loading;
      if(!actions(false)[action])return;
      await request(path,{method:'POST',body,token:getStatus().csrf_token});
    }
    catch(failure){actionError=failure.message;}
    finally{
      if(loading)await loading;
      await Promise.all([load(),refreshSetup(),refreshStatus()]);
      pending=false;reserve();render();
      if(['verify','repair','signIn'].includes(action))$('update-title').scrollIntoView({block:'start'});
    }
  }
  $('update-check').addEventListener('click',()=>void mutate('check','/api/game-update/check'));
  $('update-sign-in').addEventListener('click',()=>void mutate('signIn',data?.job?.operation==='repair'?'/api/game-update/repair/check':'/api/game-update/check',{sign_in:true}));
  $('update-verify').addEventListener('click',()=>void mutate('verify','/api/game-update/verify'));
  $('update-repair-check').addEventListener('click',()=>void mutate('repair','/api/game-update/repair/check'));
  $('update-start').addEventListener('click',()=>void mutate('start','/api/game-update/start',{check_id:data?.job?.id}));
  for(const action of ['pause','resume','cancel'])$('update-'+action).addEventListener('click',()=>void mutate(action,'/api/setup/'+action,{job_id:data?.job?.id}));
  $('update-refresh').addEventListener('click',()=>{actionError='';void load();});
  $('update-rollback').addEventListener('click',()=>{if(actions().rollback){confirmRollback=true;render();}});
  $('update-rollback-no').addEventListener('click',()=>{confirmRollback=false;render();});
  $('update-rollback-yes').addEventListener('click',()=>{if(confirmRollback)void mutate('rollback','/api/game-update/rollback');});
  setInterval(()=>{if(!document.hidden&&!pending&&(location.hash==='#updates'||setupBusy(data?.job)))void load({background:true});},1500);
  render();return {load,render};
}
