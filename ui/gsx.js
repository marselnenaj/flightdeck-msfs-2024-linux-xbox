import {t} from './i18n.js';
import {stringValue} from './state.js';

export function gsxPermissions(data,status,enabled) {
  const current=data?.runtime_path===status?.runtime?.path;
  const idle=enabled&&current&&data?.can_change===true&&status?.game?.state==='stopped';
  const available=idle&&data.state==='available';
  return {prepare:available,open:available&&data.prepared===true,
    configure:available&&data.prepared===true&&data.package_installed===true&&data.startup_found===true,
    disable:available&&data.configured===true,recover:idle&&data.can_recover===true,
    stop:enabled&&current&&data?.can_stop===true};
}

export function gsxProgress(data) {
  const active=data?.job?.state==='running';
  const steps=[data?.prepared===true,data?.package_installed===true,data?.configured===true];
  const step=steps.findIndex(value=>!value)+1;
  let title='GSX-Status wird geladen …',detail='';
  if(data?.state==='available') {
    title=steps.every(Boolean)?'GSX eingerichtet · Flugtest ausstehend':'GSX-Einrichtung';
    detail=[
      'Starte MSFS und prüfe das GSX-Menü sowie die Bodendienste. Der Flugbetrieb unter Linux ist noch nicht bestätigt.',
      'Schritt 1 von 3: Flightdeck lädt den geprüften FSDT-Installer und richtet .NET in einer Profilkopie ein.',
      'Schritt 2 von 3: Installiere und aktiviere GSX im offiziellen FSDT-Installer. Schließe ihn danach vollständig.',
      data.startup_found?'Schritt 3 von 3: Übernimm den von FSDT angelegten automatischen Start.':'Schritt 3 von 3: Die FSDT-Starteinstellung fehlt. Führe im FSDT-Installer ein Update aus und prüfe erneut.',
    ][step];
  } else if(data?.can_recover) {
    title='GSX-Einrichtung unterbrochen';detail='Stelle zuerst das bisherige Windows-Profil wieder her. Danach kannst du die Vorbereitung erneut starten.';
  } else if(data) {title='GSX ist für diese Installation nicht verfügbar';detail=data.message||'';}
  if(active){title='GSX-Einrichtung läuft …';detail=data.job.operation==='open'?'Der FSDT-Installer ist geöffnet. Beende laufende Downloads und schließe ihn danach.':'Bitte warte, bis der aktuelle Schritt abgeschlossen ist.';}
  return {title,detail,steps,step,active};
}

export function createGSX({request,getStatus,isOnline,isReserved,changed,refreshStatus,notice}) {
  const $=id=>document.getElementById(id);
  let data=null,fresh=false,loading=false,pending=false,active=false,error='';
  const permissions=()=>gsxPermissions(data,getStatus(),fresh&&isOnline()&&!isReserved()&&!pending);
  function render() {
    const allowed=permissions(),progress=gsxProgress(fresh?data:null);
    for(const name of ['prepare','open','configure','disable','recover','stop'])$('gsx-'+name).disabled=!allowed[name];
    $('gsx-state').textContent=t(progress.title);$('gsx-next').textContent=t(progress.detail);
    $('gsx-steps').hidden=!!data&&data.state!=='available';
    progress.steps.forEach((done,index)=>{
      const row=$('gsx-step-'+(index+1));
      const next=!done&&progress.step===index+1&&fresh&&data?.state==='available';
      row.dataset.status=done?'done':next?'next':'pending';
      if(next)row.setAttribute('aria-current','step');else row.removeAttribute('aria-current');
      $('gsx-step-status-'+(index+1)).textContent=t(done?'Erledigt':next?'Als Nächstes':'Noch offen');
      const button=$(['gsx-prepare','gsx-open','gsx-configure'][index]);
      button.classList.toggle('dark',next);button.classList.toggle('secondary',!next);
    });
    $('gsx-prepare').textContent=t(data?.prepared?'FSDT-Installer reparieren':'FSDT vorbereiten');
    $('gsx-stop').hidden=!data?.manager_running;
    $('gsx-disable').hidden=!data?.configured;
    $('gsx-recover').hidden=!data?.can_recover;
    $('gsx-refresh').disabled=loading||pending;
    $('gsx-message').textContent=stringValue(data?.job?.message||data?.message,'',1500);
    $('gsx-error').textContent=error;$('gsx-error').hidden=!error;
    const busy=fresh&&!active&&(data?.idle===false||data?.busy);
    $('gsx-busy').hidden=!busy;
    $('gsx-card').setAttribute('aria-busy',String(active));
  }
  async function load() {
    if(loading)return;
    loading=true;
    const runtime=getStatus()?.runtime.path;
    try {
      const result=await request('/api/gsx');
      if(runtime!==getStatus()?.runtime.path)return;
      if(!result||typeof result.state!=='string'||result.runtime_path!==(runtime||''))throw new Error(t('GSX-Status konnte nicht geladen werden.'));
      data=result;fresh=true;error='';active=result.job?.state==='running';changed(active||pending);
    } catch(failure){fresh=false;error=failure.message;}
    finally{loading=false;render();}
  }
  async function action(operation) {
    if(!permissions()[operation])return;
    const runtime=data.runtime_path;
    pending=true;changed(true);error='';render();
    try {await request('/api/gsx/'+operation,{method:'POST',body:{runtime_path:runtime},token:getStatus().csrf_token});active=true;}
    catch(failure){error=failure.message;notice(error,true);}
    finally{pending=false;changed(active);await refreshStatus();await load();}
  }
  for(const actionName of ['prepare','open','configure','disable','recover','stop'])$('gsx-'+actionName).addEventListener('click',()=>void action(actionName));
  $('gsx-refresh').addEventListener('click',()=>void load());
  setInterval(()=>{if(active||(!document.hidden&&location.hash==='#mods'))void load();},2500);
  render();
  return {load,render};
}
