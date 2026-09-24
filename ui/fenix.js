import {t} from './i18n.js';
import {stringValue} from './state.js';

export function fenixPermissions(data, status, enabled) {
  const idle=enabled&&data?.can_change===true&&data.runtime_path===status?.runtime.path&&status?.game.state==='stopped';
  return {
    install:idle&&(data.state==='available'||(data.installed===true&&data.update_available===true)),
    installer:idle&&data.installed===true,
    open:idle&&data.fenix_installed===true&&(data.installed===true||data.state==='legacy'),
    manager:idle&&data.manager_installed===true&&(data.installed===true||data.state==='legacy'),
    configure:idle&&data.installed===true&&data.fenix_installed===true&&data.settings_ready===true,
    restore:idle&&data.can_restore===true,
    stop:enabled&&data?.runtime_path===status?.runtime.path&&data?.can_stop===true,
  };
}

// Readiness follows persisted setup state, never the last job's success alone.
// Account/activation remains with the official Fenix application.
export function fenixProgress(data, status) {
  const patched=data?.state==='installed'&&data.installed===true;
  const aircraft=patched&&data.fenix_installed===true;
  const settings=aircraft&&data.settings_ready===true;
  const ready=settings&&data.configured===true;
  const step=ready?0:aircraft?(settings?4:3):patched?2:1;
  const active=data?.job?.state==='running';
  const supported=['available','installed'].includes(data?.state);
  const steps=[patched,aircraft,settings,ready].map((done,index)=>done?'done':supported&&step===index+1?'next':'pending');
  let title='Status wird geladen …',detail='';
  if(supported) {
    title=ready?'Fenix ist startbereit':'Einrichtung noch nicht abgeschlossen';
    detail=ready?'Starte MSFS 2024 ganz normal in Flightdeck. Fenix startet automatisch mit dem Spiel und wird beim Beenden mit geschlossen.':[
      'Schritt 1 von 4: Richte zuerst den Linux-Patch ein. Flightdeck lädt das geprüfte Paket automatisch herunter.',
      'Schritt 2 von 4: Lade den offiziellen Fenix-Installer herunter, wähle die EXE aus und installiere dein Flugzeug.',
      'Schritt 3 von 4: Öffne Fenix, melde dich an und schließe das Programm danach vollständig.',
      'Schritt 4 von 4: Schließe Fenix und klicke auf „Einrichtung abschließen“. Damit werden Anzeigen und automatischer Start eingerichtet.',
    ][step-1];
  } else if(data?.state==='legacy') {
    title='Vorhandene Fenix-Einrichtung';detail='Dein lokaler Fenix-Patch bleibt aktiv. Eine erneute Installation über diesen Assistenten ist nicht erforderlich.';
  } else if(data?.can_restore) {
    title='Einrichtung unterbrochen · Wiederherstellung verfügbar';detail='Öffne die Wiederherstellung unten, bevor du die Einrichtung erneut startest.';
  } else if(data) title='Für diese Runtime nicht verfügbar';
  let busy='';
  if(active&&!data.job.stopping&&['installer','open','manager'].includes(data.job.operation))
    busy='Eine Windows-Anwendung läuft noch in dieser Installation. Schließe den Fenix-Installer und Fenix nach der Anmeldung vollständig. Flightdeck aktualisiert den Status automatisch.';
  else if(!active&&status?.game.state==='external')busy='Diese Installation wird gerade verwendet. Beende MSFS oder die andere laufende Einrichtung, bevor du Fenix änderst.';
  else if(!active&&status?.game.state&&status.game.state!=='stopped')busy='MSFS läuft. Beende das Spiel, bevor du die Fenix-Einrichtung änderst.';
  else if(!active&&data?.idle===false)busy='Eine Windows-Anwendung läuft noch in dieser Installation. Schließe den Fenix-Installer und Fenix nach der Anmeldung vollständig. Flightdeck aktualisiert den Status automatisch.';
  else if(data?.busy&&!active)busy='Eine andere Einrichtung läuft. Warte, bis sie abgeschlossen ist.';
  if(active){title=data.job.stopping?'Fenix wird beendet …':'Fenix-Einrichtung läuft …';if(data.job.stopping||!['installer','open','manager'].includes(data.job.operation))detail='Bitte warte, bis der aktuelle Schritt abgeschlossen ist.';}
  return {ready:ready&&!active,supported,step,steps,title,detail,busy};
}

export function createFenix({request,getStatus,isOnline,isReserved,changed,refreshStatus,notice}) {
  const $=id=>document.getElementById(id);
  let data=null,loading=false,pending=false,fresh=false,error='',active=false;
  function permissions() {return fenixPermissions(data,getStatus(),fresh&&isOnline()&&!isReserved()&&!pending);}
  function render() {
    const allowed=permissions();
    for(const action of ['install','installer','open','manager','configure','restore','stop'])$('fenix-'+action).disabled=!allowed[action];
    $('fenix-installer').disabled||=!$('fenix-installer-path').value.trim();
    $('fenix-pick-installer').disabled=pending||active||!isOnline();
    $('fenix-pick-bundle').disabled=pending||active||!isOnline();
    $('fenix-refresh').disabled=loading;
    const progress=fenixProgress(fresh?data:null,getStatus());
    $('fenix-install').dataset.i18n=data?.update_available?'Patch aktualisieren':'Patch einrichten';
    $('fenix-install').textContent=t($('fenix-install').dataset.i18n);
    const openParent=data?.state==='legacy'||(data?.configured&&data?.settings_ready)?$('fenix-app-controls'):$('fenix-step-3');
    if($('fenix-open').parentElement!==openParent)
      openParent.insertBefore($('fenix-open'),openParent===$('fenix-app-controls')?$('fenix-stop'):null);
    $('fenix-state').textContent=t(progress.title);
    $('fenix-next').textContent=t(progress.detail);
    $('fenix-summary').classList.toggle('ready',progress.ready);
    $('fenix-overview').hidden=!progress.ready;
    $('fenix-steps').hidden=!progress.supported;
    $('fenix-configure').dataset.i18n=progress.ready?'Einstellungen erneut anwenden':'Einrichtung abschließen';
    $('fenix-configure').textContent=t($('fenix-configure').dataset.i18n);
    const actions=['install','installer','open','configure'];
    progress.steps.forEach((value,index)=>{
      const row=$('fenix-step-'+(index+1));
      row.dataset.status=value;
      if(value==='next')row.setAttribute('aria-current','step');else row.removeAttribute('aria-current');
      $('fenix-step-status-'+(index+1)).textContent=t(value==='done'?(index===2?'Einstellungen vorhanden':'Erledigt'):value==='next'?'Als Nächstes':'Noch offen');
      const button=$('fenix-'+actions[index]);
      button.classList.toggle('dark',value==='next');button.classList.toggle('secondary',value!=='next');
    });
    $('fenix-message').textContent=stringValue(data?.job?.message||data?.message,'',1500);
    $('fenix-error').textContent=error;$('fenix-error').hidden=!error;
    $('fenix-busy').textContent=t(progress.busy);$('fenix-busy').hidden=!progress.busy;
    $('fenix-app-controls').hidden=!fresh||!(data?.fenix_installed||data?.manager_installed||data?.fenix_running);
    $('fenix-app-state').textContent=t(data?.job?.stopping?'Fenix wird beendet …':data?.fenix_running?'Fenix läuft':'Fenix ist beendet');
    $('fenix-legacy').hidden=data?.state!=='legacy';
    $('fenix-card').setAttribute('aria-busy',String(active));
  }
  async function load() {
    if(loading)return;
    loading=true;
    const runtime=getStatus()?.runtime.path;
    try {
      const result=await request('/api/fenix');
      if(runtime!==getStatus()?.runtime.path)return;
      if(!result||typeof result.state!=='string'||result.runtime_path!==(runtime||''))throw new Error(t('Fenix-Status konnte nicht geladen werden.'));
      data=result;fresh=true;error='';active=data.job?.state==='running';changed(active||pending);
    } catch(failure) {fresh=false;error=failure.message;}
    finally {loading=false;render();}
  }
  async function action(operation) {
    if(!permissions()[operation])return;
    if(operation==='restore'&&!window.confirm(t('Runner und Windows-Profil auf den Stand vor dem Patch zurücksetzen? Das aktuelle Profil bleibt als Sicherung erhalten.')))return;
    const body={};
    if(operation==='install')body.bundle_path=$('fenix-bundle-path').value.trim();
    if(operation==='installer')body.installer_path=$('fenix-installer-path').value.trim();
    pending=true;changed(true);error='';render();
    try {await request('/api/fenix/'+operation,{method:'POST',body,token:getStatus().csrf_token});active=true;}
    catch(failure) {error=failure.message;notice(error,true);}
    finally {pending=false;changed(active);await refreshStatus();await load();}
  }
  async function pick(kind) {
    if(pending||active||!isOnline())return;
    try {
      const result=await request('/api/fenix/pick',{method:'POST',body:{kind},token:getStatus().csrf_token});
      if(!result.cancelled&&typeof result.path==='string')$('fenix-'+kind+'-path').value=result.path;
    } catch(failure) {error=failure.message;}
    render();
  }
  for(const operation of ['install','installer','open','manager','configure','restore','stop'])$('fenix-'+operation).addEventListener('click',()=>void action(operation));
  for(const kind of ['installer','bundle'])$('fenix-pick-'+kind).addEventListener('click',()=>void pick(kind));
  $('fenix-installer-path').addEventListener('input',render);
  $('fenix-refresh').addEventListener('click',()=>void load());
  setInterval(()=>{if(active||(!document.hidden&&location.hash==='#mods'))void load();},2500);
  render();
  return {load,render};
}
