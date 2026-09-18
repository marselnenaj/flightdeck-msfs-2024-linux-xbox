import {t} from './i18n.js';
import {stringValue, formatBytes, formatCount, formatDate, normalizeAutomatic, automaticBusy} from './state.js';

const count=value=>Number.isSafeInteger(value)&&value>=0;
const running=data=>data?.job?.state==='running';
const planCounts=['container_count','blob_count','total_bytes','local_container_count','add_count','replace_count','delete_count','unchanged_count','conflict_count'];

export function normalizeCloud(raw) {
  const invalid=()=>new Error(t('Der Cloud-Status ist nicht verfügbar.'));
  if(!raw||typeof raw.available!=='boolean'||!['download_only','download_and_import','manual_sync','automatic_sync'].includes(raw.mode)||raw.sync_supported!==['manual_sync','automatic_sync'].includes(raw.mode))throw invalid();
  if(raw.restore_id!=null&&(typeof raw.restore_id!=='string'||!raw.restore_id||raw.restore_id.length>128))throw invalid();
  let plan=null;
  if(raw.plan!=null) {
    const p=raw.plan;
    if(raw.mode==='download_only'||typeof p.id!=='string'||!p.id||p.id.length>128||typeof p.local_exists!=='boolean'||planCounts.some(key=>!count(p[key])))throw invalid();
    plan={id:p.id,local_exists:p.local_exists,...Object.fromEntries(planCounts.map(key=>[key,p[key]]))};
  }
  let job=null;
  if(raw.job!==null) {
    const value=raw.job;
    if(!value||typeof value.id!=='string'||!value.id||value.id.length>128||
      !['check','download','prepare-import','import','upload','restore'].includes(value.operation)||!['running','succeeded','failed','cancelled'].includes(value.state))throw invalid();
    let result=null;
    if(value.state==='succeeded') {
      const r=value.result;
      if(!r||!count(r.container_count)||!count(r.total_bytes)||!(r.blob_count===null||count(r.blob_count))||
        typeof r.downloaded!=='boolean'||typeof r.rechecked!=='boolean'||
        (['download','prepare-import','import'].includes(value.operation)&&(!r.downloaded||!r.rechecked||r.blob_count===null))||
        (value.operation==='check'&&r.downloaded)||
        (value.operation==='prepare-import'&&r.prepared_for_import!==true)||
        (['import','restore'].includes(value.operation)&&(typeof r.imported!=='boolean'||typeof r.durability_confirmed!=='boolean'))||
        (value.operation==='restore'&&(r.restored!==true||r.downloaded||r.blob_count===null))||
        (value.operation==='upload'&&(r.uploaded!==true||r.downloaded||!r.rechecked||r.blob_count===null||!count(r.changed_containers)||typeof r.lease_released!=='boolean'||typeof r.baseline_saved!=='boolean')))throw invalid();
      result={container_count:r.container_count,blob_count:r.blob_count,total_bytes:r.total_bytes,downloaded:r.downloaded,
        imported:r.imported===true,restored:r.restored===true,uploaded:r.uploaded===true,prepared_for_import:r.prepared_for_import===true};
    }
    job={id:value.id,operation:value.operation,state:value.state,result,
      recovery_required:value.recovery_required===true,message:stringValue(value.message,'',1200),finished_at:value.finished_at};
  }
  return {available:raw.available,can_check:raw.can_check===true,can_download:raw.can_download===true,
    can_prepare_import:raw.can_prepare_import===true,can_import:raw.can_import===true,
    can_upload:['manual_sync','automatic_sync'].includes(raw.mode)&&raw.can_upload===true,can_restore:raw.can_restore===true,
    restore_id:raw.restore_id??null,sync_supported:raw.sync_supported,
    automatic:normalizeAutomatic(raw.automatic),can_cancel:raw.can_cancel===true,job,plan};
}

export function cloudActions(data,status,{online=false,fresh=false,pending=false,reserved=false}={}) {
  const enabled=!!data&&online&&fresh&&!pending&&!!status?.csrf_token&&status?.runtime.configured===true;
  const idle=enabled&&data.available&&!running(data)&&!reserved&&!automaticBusy(status)&&status?.game.state==='stopped';
  return {check:idle&&data.can_check,download:idle&&data.can_download,
    'prepare-import':idle&&data.can_prepare_import,import:idle&&data.can_import&&!!data.plan,
    upload:idle&&data.can_upload&&!!data.plan,restore:idle&&data.can_restore&&!!data.restore_id,
    'discard-plan':idle&&!!data.plan,
    cancel:enabled&&running(data)&&data.can_cancel};
}

export function cloudTitle(data) {
  if(!data)return t('Cloud-Status wird geladen …');
  if(running(data))return t(data.job.operation==='upload'?'Spielstände werden hochgeladen':data.job.operation==='restore'?'Spielstände werden wiederhergestellt':data.job.operation==='import'?'Cloud-Spielstände werden übernommen':data.job.operation==='prepare-import'?'Spielstände werden verglichen':data.job.operation==='download'?'Cloud-Kopie wird heruntergeladen':'Cloud-Spielstände werden geprüft');
  if(data.job?.recovery_required)return t('Upload nicht vollständig bestätigt');
  if(data.job?.state==='failed')return t('Cloud-Anfrage fehlgeschlagen');
  if(data.job?.state==='cancelled')return t('Cloud-Anfrage abgebrochen');
  if(data.plan)return t('Cloud und lokaler Spielstand');
  if(data.job?.result)return t(data.job.result.restored?'Lokale Spielstände wiederhergestellt':data.job.result.uploaded?'Xbox-Cloud aktualisiert':data.job.operation==='import'?(data.job.result.imported?'Cloud-Spielstände übernommen':'Spielstände stimmen überein'):data.job.result.downloaded?'Cloud-Kopie gespeichert':data.job.result.container_count===0?'Keine Cloud-Spielstände gefunden':'Cloud-Spielstände gefunden');
  return t(data.available?'Noch nicht geprüft':'Cloud-Zugriff nicht verfügbar');
}

export function automaticActions(status,{online=false,pending=false,reserved=false}={}) {
  const cloud=status?.cloud;
  const enabled=!!cloud?.enabled&&!!cloud.request_id&&online&&!pending&&!reserved&&!!status?.csrf_token&&status.runtime.configured&&status.game.state==='stopped';
  return {retry:enabled&&cloud.state==='attention'&&cloud.can_retry===true,
    'play-local':enabled&&cloud.state==='attention'&&!cloud.conflict&&cloud.can_play_local===true,
    'cancel-auto':enabled&&['syncing','attention'].includes(cloud.state)&&cloud.can_cancel===true,
    cloud:enabled&&cloud.state==='attention'&&cloud.conflict&&cloud.can_retry===true,
    local:enabled&&cloud.state==='attention'&&cloud.conflict&&cloud.can_retry===true};
}
export function automaticTitle(cloud) {
  if(!cloud)return t('Cloud-Status wird geladen …');
  if(!cloud.enabled)return t('Automatischer Cloud-Abgleich nicht verfügbar');
  if(cloud.state==='attention')return t(cloud.conflict?'Spielstand auswählen':'Cloud-Abgleich braucht Aufmerksamkeit');
  return t({idle:'Automatisch vor und nach dem Spielen',syncing:'Spielstände werden synchronisiert',playing:'Abgleich nach dem Beenden',synced:'Spielstände synchronisiert',local:'Diese Sitzung bleibt lokal'}[cloud.state]);
}

export function createCloudSaves({request,getStatus,isOnline,isReserved,refreshStatus,changed}) {
  const $=id=>document.getElementById(id);
  const text=(id,value)=>{if($(id).textContent!==value)$(id).textContent=value;};
  let data=null,loading=null,foreground=false,pending=false,fresh=false,error='',actionError='',loadedRuntime,reservation=false;
  const sameRuntime=()=>loadedRuntime===getStatus()?.runtime.path;
  const current=()=>fresh&&sameRuntime();
  const actions=(busy=pending)=>cloudActions(data,getStatus(),{online:isOnline(),fresh:current(),pending:busy,reserved:isReserved()});
  function reserve() {
    const value=pending||(sameRuntime()&&running(data));
    if(value!==reservation){reservation=value;changed(value);}
  }
  function render() {
    renderAutomatic();
    const visible=sameRuntime()?data:null,job=visible?.job,result=job?.result,plan=visible?.plan,allowed=actions();
    text('cloud-status',cloudTitle(visible));
    text('cloud-mode',t(visible?.sync_supported?'Manuell synchronisieren':'Vergleichen und übernehmen'));
    text('cloud-limit',t(visible?.sync_supported?'Vergleiche lokale und Xbox-Cloud-Spielstände und wähle die Übertragungsrichtung. Flightdeck sichert den bisherigen Stand und prüft die übertragenen Daten. Der Simulator muss dabei geschlossen sein.':'Vergleiche deine Xbox-Cloud-Spielstände mit dem lokalen Stand. Vor einer Übernahme sichert Flightdeck deine bisherigen Spielstände. Hochladen ist noch nicht verfügbar.'));
    text('cloud-message',job?.message||t(visible?.available===false
      ?'Für diese Installation fehlt die Cloud-Komponente. Installiere das aktuelle vollständige Flightdeck-Paket.'
      :'Prüfe die Xbox-Cloud-Spielstände des angemeldeten Spielprofils oder lade eine getrennte Kopie auf diesen Rechner.'));
    const failure=actionError||error;
    text('cloud-error',failure);$('cloud-error').hidden=!failure;
    $('cloud-stale').hidden=!visible||current()||foreground;
    $('cloud-progress').hidden=!running(visible);
    $('cloud-results').hidden=!result;
    text('cloud-containers',result?formatCount(result.container_count):'—');
    text('cloud-blobs',result?.blob_count!==null&&result?formatCount(result.blob_count):t('Nicht ermittelt'));
    text('cloud-bytes',result?formatBytes(result.total_bytes):'—');
    const date=job?.state==='succeeded'?formatDate(job.finished_at):null;
    text('cloud-time',date?t('Geprüft: {time}',{time:date}):'');$('cloud-time').hidden=!date;
    $('cloud-copy-note').hidden=!result?.downloaded||result.imported||!!plan;
    $('cloud-plan').hidden=!plan;
    text('cloud-plan-summary',plan?t('{cloud} Container in der Cloud · {local} lokal',{cloud:formatCount(plan.container_count),local:formatCount(plan.local_container_count)}):'');
    text('cloud-plan-changes',plan?t('{add} hinzufügen · {replace} ersetzen · {remove} entfernen · {same} unverändert',{add:formatCount(plan.add_count),replace:formatCount(plan.replace_count),remove:formatCount(plan.delete_count),same:formatCount(plan.unchanged_count)}):'');
    text('cloud-plan-conflicts',plan?.conflict_count?t('{count} Container unterscheiden sich ohne gemeinsamen Vergleichsstand. Wähle bewusst, welchen Stand du verwenden möchtest.',{count:formatCount(plan.conflict_count)}):'');
    $('cloud-plan-conflicts').hidden=!plan?.conflict_count;
    $('cloud-empty-warning').hidden=!plan||plan.container_count!==0||plan.local_container_count===0;
    $('cloud-upload-choice').hidden=!visible?.sync_supported;
    text('cloud-upload-changes',plan?t('Beim Hochladen: {add} Cloud-Container hinzufügen · {replace} ersetzen · {remove} entfernen',{add:formatCount(plan.delete_count),replace:formatCount(plan.replace_count),remove:formatCount(plan.add_count)}):'');
    $('cloud-local-empty-warning').hidden=!plan||plan.local_container_count!==0||plan.container_count===0;
    for(const action of ['check','download','prepare-import','import','upload','restore','discard-plan','cancel']) {
      const button=$('cloud-'+action),disabled=!allowed[action]||foreground;
      if(button.disabled!==disabled)button.disabled=disabled;
    }
    $('cloud-cancel').hidden=!running(visible);
    $('cloud-restore').hidden=!visible?.restore_id;
    $('cloud-refresh').disabled=foreground||pending;
    text('cloud-refresh',t(foreground?'Status wird geladen …':'Status neu laden'));
    const busy=getStatus()?.game.state!=='stopped'||automaticBusy(getStatus())||isReserved();
    $('cloud-busy').hidden=running(visible)||!busy;
    text('cloud-busy',t('Beende den Simulator und schließe laufende Einrichtungen ab, bevor du auf Cloud-Spielstände zugreifst.'));
  }
  function renderAutomatic() {
    const status=getStatus(),cloud=status?.cloud;
    const allowed=automaticActions(status,{online:isOnline(),pending,reserved:isReserved()||(sameRuntime()&&running(data))});
    for(const prefix of ['overview-cloud','auto-cloud']) {
      text(prefix+'-title',automaticTitle(cloud));
      text(prefix+'-message',(cloud?.enabled&&cloud.message)||t(cloud?.enabled?'Flightdeck gleicht deine Xbox-Spielstände vor dem Start und nach dem Beenden ab. Vor Änderungen bleibt eine lokale Sicherung erhalten.':'Verbinde eine Installation mit Cloud-Unterstützung, um deine Spielstände automatisch abzugleichen.'));
      $(prefix+'-progress').hidden=cloud?.state!=='syncing';
      $(prefix+'-conflict').hidden=!cloud?.conflict;
      const counts=cloud?.summary;
      text(prefix+'-counts',counts&&Number.isSafeInteger(counts.container_count)&&Number.isSafeInteger(counts.local_container_count)?t('{cloud} Spielstandbereiche in der Cloud · {local} auf diesem Rechner',{cloud:formatCount(counts.container_count),local:formatCount(counts.local_container_count)}):'');
      const date=formatDate(cloud?.last_synced_at);
      text(prefix+'-time',date?t('Zuletzt synchronisiert: {time}',{time:date}):'');
      text(prefix+'-error',actionError);$(prefix+'-error').hidden=!actionError;
      for(const action of ['retry','play-local','cancel-auto','cloud','local']) {
        const button=$(prefix+'-'+action);
        button.hidden=!allowed[action]&&!(cloud?.conflict&&['cloud','local'].includes(action));
        button.disabled=!allowed[action];
      }
    }
  }
  async function mutateAutomatic(action) {
    const allowed=()=>automaticActions(getStatus(),{online:isOnline(),pending:false,reserved:isReserved()||(sameRuntime()&&running(data))});
    if(pending||!allowed()[action])return;
    const id=getStatus().cloud.request_id,runtime=getStatus().runtime.path;
    pending=true;actionError='';reserve();render();
    try {
      // Re-read status after any old poll: a stale choice must never target a new session.
      await refreshStatus();
      if(!allowed()[action]||getStatus().cloud?.request_id!==id||getStatus().runtime.path!==runtime)return;
      const choice=['cloud','local'].includes(action);
      await request('/api/cloud-saves/'+(choice?'resolve':action),{method:'POST',body:choice?{request_id:id,choice:action}:{request_id:id},token:getStatus().csrf_token});
    } catch(failure){actionError=failure.message;}
    finally {
      await refreshStatus();
      pending=false;reserve();render();
    }
  }
  async function load({background=false}={}) {
    if(loading)return loading;
    const runtime=getStatus()?.runtime.path;foreground=!background;
    loading=(async()=>{
      try{data=normalizeCloud(await request('/api/cloud-saves'));loadedRuntime=runtime;fresh=true;error='';}
      catch(failure){fresh=false;error=failure.message;}
      finally{loading=null;foreground=false;reserve();render();}
    })();
    render();return loading;
  }
  async function mutate(action) {
    if(!actions()[action])return;
    const jobId=data?.job?.id;
    const planId=data?.plan?.id;
    const restoreId=data?.restore_id;
    pending=true;actionError='';reserve();render();
    try {
      if(loading)await loading;
      if(!actions(false)[action]||(action==='cancel'&&data?.job?.id!==jobId)||(['import','upload','discard-plan'].includes(action)&&data?.plan?.id!==planId)||(action==='restore'&&data?.restore_id!==restoreId))return;
      const body=action==='cancel'?{job_id:jobId}:action==='import'?{plan_id:planId,choice:'cloud'}:action==='upload'?{plan_id:planId,choice:'local'}:action==='restore'?{backup_id:restoreId}:action==='discard-plan'?{plan_id:planId}:{};
      await request('/api/cloud-saves/'+action,{method:'POST',body,token:getStatus().csrf_token});
    } catch(failure){actionError=failure.message;}
    finally {
      if(loading)await loading;
      await Promise.all([load(),refreshStatus()]);
      pending=false;reserve();render();
    }
  }
  for(const action of ['check','download','prepare-import','import','upload','restore','discard-plan','cancel'])$('cloud-'+action).addEventListener('click',()=>void mutate(action));
  for(const prefix of ['overview-cloud','auto-cloud'])for(const action of ['retry','play-local','cancel-auto','cloud','local'])$(prefix+'-'+action).addEventListener('click',()=>void mutateAutomatic(action));
  window.addEventListener('flightdeck-languagechange',()=>{actionError='';render();});
  $('cloud-refresh').addEventListener('click',()=>{actionError='';void load();});
  setInterval(()=>{if(!document.hidden&&!pending&&(location.hash==='#saves'||(sameRuntime()&&running(data))))void load({background:true});},1500);
  render();return {load,render};
}
