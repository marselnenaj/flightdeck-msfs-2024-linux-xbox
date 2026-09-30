import {t} from './i18n.js';
import {formatDate} from './state.js';

const stages={runtime:'Store-Komponenten',sign_in:'Microsoft-Anmeldung',account:'Gespeicherte Anmeldung',catalog:'Produktkatalog',license:'Spiellizenz',library:'Bibliothek',window:'Store-Fenster'};
const codes={
  signing_in:'Melde dich im Microsoft-Fenster mit demselben Konto an. Danach wird die Sitzung erneut geprüft.',
  checking:'Wird geprüft …', available:'Erreichbar', verified:'Geprüft', local_session:'Anmeldung vorhanden', visible:'Anzeige bestätigt',
  sign_in_required:'Bitte in Flightdeck anmelden.', expired:'Die Anmeldung ist abgelaufen. Bitte erneut anmelden.',
  not_licensed:'Für dieses Konto wurde keine gültige Spiellizenz bestätigt.', unsupported:'Diese Antwort wird noch nicht unterstützt.',
  invalid_config:'Die Spielkonfiguration ist unvollständig. Prüfe die Installation.', connection:'Die Store-Abfrage ist fehlgeschlagen. Prüfe die Verbindung und den Diagnosebericht.',
  timeout:'Zeitüberschreitung. Prüfe die Verbindung oder schließe das Testfenster.', account_changed:'Das angemeldete Konto hat sich während der Prüfung geändert. Starte die Prüfung erneut.',
  keyring:'Die gespeicherte Anmeldung ist nicht zugänglich. Entsperre den Schlüsselbund.', error:'Dieser Prüfschritt konnte nicht abgeschlossen werden.',
  cancelled:'Abgebrochen', runtime_update:'Aktualisiere die Store-Komponenten mit Flightdeck und starte die Prüfung erneut.', incomplete:'Nicht geprüft',
};
export function storeCheckActions(job,status,{online=true,reserved=false,pending=false}={}) {
  const running=job?.state==='running';
  return {start:!!(online&&!reserved&&!pending&&status?.csrf_token&&status.runtime.configured&&status.game.state==='stopped'&&!status.setup?.busy&&!['syncing','playing'].includes(status.cloud?.state)&&!running),
    cancel:!!(online&&!pending&&running),running};
}
export function createStoreCheck({request,getStatus,isOnline,isReserved,refreshStatus,changed}) {
  const $=id=>document.getElementById(id);
  let data={job:null},pending=false,loading=null,error='',reserved=false;
  const allowed=()=>storeCheckActions(data.job,getStatus(),{online:isOnline(),reserved:isReserved(),pending});
  function reserve(){const next=pending||data.job?.state==='running';if(next!==reserved){reserved=next;changed(next);}}
  function render(){
    const job=data.job, actions=allowed();
    $('store-check-start').disabled=!actions.start;
    $('store-check-sign-in').disabled=!actions.start;
    $('store-check-cancel').hidden=!actions.running;
    $('store-check-cancel').disabled=!actions.cancel;
    $('store-check-busy').hidden=actions.start||actions.running;
    const labels={running:'Store-Prüfung läuft …',passed:'Store-Basisprüfungen erfolgreich',failed:'Store-Prüfung mit Fehlern',cancelled:'Store-Prüfung abgebrochen',incomplete:'Store-Prüfung unvollständig'};
    $('store-check-status').textContent=job?t(labels[job.state]||'Store-Prüfung unvollständig')+(job.finished_at?' · '+formatDate(job.finished_at):''):t('Noch keine Store-Prüfung durchgeführt.');
    if(job?.operation==='recover'&&actions.running)$('store-check-status').textContent=t('Anmeldung wird erneuert und geprüft …');
    $('store-check-error').textContent=error;$('store-check-error').hidden=!error;
    $('store-check-steps').replaceChildren(...(job?.steps||[]).filter(row=>Object.hasOwn(stages,row.stage)).map(row=>{
      const li=document.createElement('li');li.className='check-row';
      const marker=document.createElement('span');marker.className='check-indicator'+(row.state==='passed'?'':row.state==='failed'?' failed':' unknown');marker.textContent=row.state==='passed'?'✓':row.state==='failed'?'!':'·';marker.setAttribute('aria-hidden','true');
      const content=document.createElement('div');content.className='check-text';const label=document.createElement('strong');label.textContent=t(stages[row.stage]);
      const detail=document.createElement('p');
      detail.textContent=(row.stage==='window'&&row.state==='running'?t('Bestätige im Testfenster, ob Text und Schaltflächen sichtbar sind.'):row.state==='pending'?t('Ausstehend'):t(codes[row.code]||codes.incomplete));
      content.append(label,detail);li.append(marker,content);return li;
    }));
  }
  async function load(){
    if(loading)return loading;
    loading=(async()=>{try{data=await request('/api/store-check');error='';}catch(e){error=e.message;}finally{loading=null;reserve();render();}})();return loading;
  }
  async function action(path,body={}){
    pending=true;error='';reserve();render();
    try{if(loading)await loading;data=await request(path,{method:'POST',body,token:getStatus()?.csrf_token});}
    catch(e){error=e.message;}
    finally{pending=false;reserve();await refreshStatus();render();}
  }
  $('store-check-start').addEventListener('click',()=>{if(allowed().start)void action('/api/store-check/start');});
  $('store-check-sign-in').addEventListener('click',()=>{if(allowed().start)void action('/api/store-check/sign-in',{job_id:data.job?.id});});
  $('store-check-cancel').addEventListener('click',()=>{if(allowed().cancel)void action('/api/store-check/cancel',{job_id:data.job.id});});
  return {render,load,poll:()=>{if(reserved||location.hash==='#diagnostics')void load();}};
}
