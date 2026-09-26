import {t} from './i18n.js';
import {formatBytes} from './state.js';

export function maintenanceActions(job, status, {online=true, reserved=false, pending=false}={}) {
  const idle=online&&!reserved&&!pending&&!!status?.csrf_token&&status.runtime.configured&&status.game.state==='stopped'&&!['syncing','playing'].includes(status.cloud?.state);
  const busy=['checking','running'].includes(job?.state);
  return {preview:!!idle&&!busy, confirm:!!idle&&job?.state==='ready'&&job.runtime_path===status.runtime.path,
    discard:!pending&&job?.state==='ready', busy};
}

export function createMaintenance({request,getStatus,isOnline,isReserved,isSetupActive,refreshStatus,changed}) {
  const $=id=>document.getElementById(id);
  let data={job:null}, pending=false, loading=null, error='', actionError='', reserved=false;
  const options=()=>({online:isOnline(),reserved:isReserved(),pending});
  function reserve() {
    const next=pending||maintenanceActions(data.job,getStatus()).busy;
    if(next!==reserved){reserved=next;changed(next);}
  }
  function render() {
    const job=data.job, status=getStatus(), allowed=maintenanceActions(job,status,options());
    $('maintenance-card').hidden=isSetupActive()||(!status?.runtime.configured&&!job);
    $('maintenance-game').textContent=status?.runtime.game_name||t('Keine Installation ausgewählt');
    $('maintenance-reset').disabled=!allowed.preview;
    $('maintenance-uninstall').disabled=!allowed.preview;
    $('maintenance-restore').hidden=!data.can_restore;
    $('maintenance-restore').disabled=!allowed.preview;
    $('maintenance-delete-packages').disabled=pending||allowed.busy;
    if(!$('maintenance-delete-packages').checked)$('maintenance-keep-data').checked=true;
    $('maintenance-keep-data').disabled=pending||allowed.busy||!$('maintenance-delete-packages').checked;
    const show=job?.state==='ready'&&job.runtime_path===status?.runtime.path;
    $('maintenance-preview').hidden=!show;
    $('maintenance-confirm').disabled=!allowed.confirm;
    $('maintenance-discard').disabled=!allowed.discard;
    $('maintenance-progress').hidden=!allowed.busy;
    $('maintenance-message').textContent=job?.state==='complete'?`${job.game_name}: ${job.message}`:job?.message||'';
    $('maintenance-error').textContent=actionError||error||job?.error||'';
    $('maintenance-error').hidden=!$('maintenance-error').textContent;
    $('maintenance-busy').hidden=!!allowed.preview||allowed.busy||!status?.runtime.configured;
    $('maintenance-backup').hidden=!job?.backup_path;
    $('maintenance-backup-path').textContent=job?.backup_path||'';
    if(show) {
      const uninstall=job.operation==='uninstall';
      $('maintenance-confirm').classList.toggle('danger',uninstall&&job.delete_packages);
      $('maintenance-preview-title').textContent=t(uninstall?'Spiel deinstallieren?':job.operation==='restore'?'Vorherige Spielumgebung wiederherstellen?':'Spielumgebung zurücksetzen?');
      $('maintenance-preview-game').textContent=job.game_name;
      $('maintenance-runtime').textContent=job.runtime_path;
      $('maintenance-package-list').replaceChildren(...(job.packages||[]).map(path=>{const li=document.createElement('li');li.textContent=path;return li;}));
      $('maintenance-package-section').hidden=!uninstall||!job.delete_packages;
      $('maintenance-package-size').textContent=formatBytes(job.package_bytes);
      $('maintenance-effects').textContent=t(uninstall?(job.keep_data?'Einstellungen, lokale Spielstände und die übrige Installation werden in einem Sicherungsordner behalten.':'Die gesamte ausgewählte Installation mit lokalen Spielständen und Einstellungen wird dauerhaft gelöscht.'):
        job.operation==='restore'?'Die letzte gesicherte Umgebung wird wieder aktiviert. Die aktuelle Umgebung bleibt ebenfalls erhalten.':
        'Eine frische Windows-Umgebung ersetzt die bisherige. Die alte Umgebung wird gesichert. Basisspiel und lokale Spielstände bleiben erhalten; Zusatzprogramme müssen neu eingerichtet werden.');
      $('maintenance-retained').textContent=t(uninstall&&!job.delete_packages?'Die Spieldateien bleiben erhalten. Die Installation wird nur aus Flightdeck entfernt.':'Externe Add-ons, Runner, Anmeldedaten außerhalb des Installationsordners und Xbox-Cloud-Spielstände bleiben erhalten.');
      $('maintenance-confirm').textContent=t(uninstall?'Jetzt deinstallieren':job.operation==='restore'?'Jetzt wiederherstellen':'Jetzt zurücksetzen');
    }
  }
  async function load() {
    if(loading)return loading;
    loading=(async()=>{
      try {data=await request('/api/maintenance');error='';}
      catch(failure){error=failure.message;}
      finally {loading=null;reserve();render();}
    })();
    return loading;
  }
  async function action(path,body) {
    pending=true;error='';actionError='';reserve();render();
    try {
      if(loading)await loading;
      data=await request(path,{method:'POST',body,token:getStatus()?.csrf_token});
    }
    catch(failure){actionError=failure.message;}
    finally {pending=false;reserve();await refreshStatus();render();}
  }
  for(const operation of ['reset','restore','uninstall'])$('maintenance-'+operation).addEventListener('click',()=>{
    if(!maintenanceActions(data.job,getStatus(),options()).preview)return;
    void action('/api/maintenance/preview',{operation,keep_data:$('maintenance-keep-data').checked,delete_packages:$('maintenance-delete-packages').checked});
  });
  $('maintenance-confirm').addEventListener('click',()=>{
    if(maintenanceActions(data.job,getStatus(),options()).confirm)void action('/api/maintenance/start',{job_id:data.job.id,confirmed:true});
  });
  $('maintenance-discard').addEventListener('click',()=>void action('/api/maintenance/discard',{job_id:data.job.id}));
  for(const id of ['maintenance-keep-data','maintenance-delete-packages'])$(id).addEventListener('change',()=>{
    if(data.job?.state==='ready')void action('/api/maintenance/discard',{job_id:data.job.id});
    render();
  });
  return {render,load,poll:()=>{if(reserved||location.hash==='#installation')void load();}};
}
