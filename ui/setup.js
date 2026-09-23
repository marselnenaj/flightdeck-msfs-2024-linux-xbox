import {t, plural, locale} from './i18n.js';
import {stringValue, normalizeChecks, normalizeTransfer, renderJobProgress} from './state.js';

const ACTIVE = new Set(['checking', 'ready', 'installing']);
const STATES = new Set([...ACTIVE, 'complete', 'failed', 'cancelled']);
// ISO 3166-1 alpha-2 codes, IANA tzdb public-domain table (2025-07-01):
// https://data.iana.org/time-zones/tzdb/iso3166.tab
// Identifiers only; availability is still checked against the actual Store.
const REGIONS = new Set('AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS YE YT ZA ZM ZW'.split(' '));
export function normalizeRegion(code) {
  const value = stringValue(code, '', 32).trim().toUpperCase();
  return REGIONS.has(value) ? value : '';
}
export function normalizeSetup(raw) {
  if (!raw || typeof raw !== 'object' || typeof raw.available !== 'boolean') throw new Error(t('Einrichtungsstatus nicht verfügbar.'));
  const source = raw.job;
  if (source && (!STATES.has(source.state) || !stringValue(source.id))) throw new Error(t('Unbekannter Einrichtungsstatus.'));
  const job = source ? {
    id: stringValue(source.id, '', 256), state: source.state,
    mode:['install','existing','prepare','update'].includes(source.mode)?source.mode:null,
    operation:['update','repair','verify'].includes(source.operation)?source.operation:null,
    phase: stringValue(source.phase, '', 100), message: stringValue(source.message, '', 1200),
    failure_phase: stringValue(source.failure_phase, '', 100),
    can_pause: source.can_pause===true, can_resume: source.can_resume===true,
    error: stringValue(source.error, '', 1200), checks: normalizeChecks(source.checks),
    progress: typeof source.progress === 'number' && Number.isFinite(source.progress) && source.progress >= 0 && source.progress <= 100 ? source.progress : null,
    transfer: normalizeTransfer(source.transfer),
    runtime_path: stringValue(source.runtime_path), market: normalizeRegion(source.market),
    game_id: ['msfs2020','msfs2024'].includes(source.game_id)?source.game_id:'msfs2024',
  } : null;
  return {available: raw.available, install_available:raw.install_available===true, install_unavailable_reason:stringValue(raw.install_unavailable_reason,'',1200), prepare_available: raw.prepare_available === true, directory_picker: raw.directory_picker === true,
    prepare_unavailable_reason: stringValue(raw.prepare_unavailable_reason, '', 1200),
    defaults: raw.defaults && typeof raw.defaults === 'object' ? raw.defaults : {}, job};
}
export function setupBusy(job) { return !!job && ACTIVE.has(job.state) && !(job.mode==='update'&&job.state==='ready'); }

export function downloadActions(job) {
  return {pause:job?.state==='installing'&&job.phase==='download'&&job.can_pause===true,
    resume:job?.state==='installing'&&job.phase==='paused'&&job.can_resume===true};
}

export function regionName(code) {
  const value=normalizeRegion(code);
  if(!value)return t('Region auswählen …');
  try { return new Intl.DisplayNames([locale()],{type:'region'}).of(value)+' ('+value+')'; }
  catch { return value; }
}
export function regionOptions() {
  const collator = new Intl.Collator(locale());
  return [...REGIONS].map(code=>({code,label:regionName(code)})).sort((a,b)=>collator.compare(a.label,b.label));
}

export function installationHelp(job) {
  if(job?.mode!=='install')return '';
  if(job.state==='failed') {
    if(job.failure_phase==='authentication')return t('Prüfe die Angaben erneut. Melde dich danach im Microsoft-Fenster mit dem Konto an, dem deine Xbox-PC-Version gehört.');
    if(job.failure_phase==='download')return t('Prüfe deine Internetverbindung und den freien Speicherplatz. Die Meldung unten beschreibt den abgebrochenen Schritt.');
    return t('Die Meldung unten nennt, was fehlt. Fehlende Linux-Komponenten kannst du über die Softwareverwaltung installieren. Prüfe danach die Angaben erneut.');
  }
  if(job.state==='installing'&&job.phase==='authentication')return t('Das Microsoft-Anmeldefenster öffnet sich separat. Wähle dort das Konto, mit dem du die Xbox-PC-Version gekauft hast.');
  if(job.state==='installing'&&job.phase==='paused')return t('Der Download ist pausiert. Lass Flightdeck geöffnet, um ihn später fortzusetzen. Fertige, geprüfte Dateien bleiben erhalten; die laufende Datei beginnt erneut.');
  if(downloadActions(job).pause)return t('Du kannst den Download pausieren und in dieser geöffneten Flightdeck-Sitzung fortsetzen. Fertige, geprüfte Dateien bleiben erhalten; die laufende Datei beginnt erneut.');
  return '';
}

export function createSetup({request, getStatus, isOnline, renderChecks, notice, refreshStatus, changed}) {
  const $ = id => document.getElementById(id);
  const fieldIds = {runtime_path:'runtime-path', artifacts_path:'setup-artifacts', game_path:'setup-game',
    runner_path:'setup-runner', prefix_path:'setup-prefix', destination_path:'setup-destination',
    market:'setup-market', media_plugins_path:'setup-media'};
  let data = null, online = false, pending = false, initialized = false, polling = null;
  let ignoredId = null, completedId = null, errorMessage = '', mode = 'existing';
  let discovery = null, discovering = false, discoveryError = false;
  let connectAfterCheck = false, ownCheckId = null;
  let regionLocale = '';
  let suggestedDestination = '';
  let preferredSetup = null;
  function setEdition(gameId) {
    if(!['msfs2020','msfs2024'].includes(gameId))return;
    $('setup-game-id').value=gameId;
    const replacement=suggestedDestination.replace(/\/(msfs2020|msfs2024)$/,`/${gameId}`);
    for(const id of ['install-destination','setup-destination']){
      const current=$(id).value;
      if(current===suggestedDestination||/\/(msfs2020|msfs2024)$/.test(current))$(id).value=current.replace(/\/(msfs2020|msfs2024)$/,`/${gameId}`);
    }
    suggestedDestination=replacement;
  }
  function applyPreferredSetup() {
    if(!preferredSetup||!initialized)return;
    const choice=preferredSetup;preferredSetup=null;
    mode=choice.mode;
    if(choice.mode==='install')setEdition(choice.gameId);
    else $('runtime-path').value=choice.path;
  }
  function focusSetup(choice) {
    preferredSetup=choice;
    if(!initialized)return;
    if(setupBusy(data?.job))return;
    if(data?.job){ignoredId=data.job.id;data={...data,job:null};}
    applyPreferredSetup();render();
  }
  function renderRegions() {
    if(regionLocale===locale())return;
    regionLocale=locale();
    const choices=regionOptions();
    for(const id of ['install-market','setup-market']) {
      const select=$(id), selected=select.value;
      const placeholder=document.createElement('option');placeholder.value='';placeholder.textContent=t('Region auswählen …');
      const options=choices.map(({code,label})=>{const option=document.createElement('option');option.value=code;option.textContent=label;return option;});
      select.replaceChildren(placeholder,...options);select.value=normalizeRegion(selected);
    }
  }
  renderRegions();
  const candidateRows = [];
  function renderDiscovery(locked) {
    $('setup-discovery').hidden = mode !== 'existing';
    $('setup-manual').hidden = mode !== 'existing';
    $('setup-discover').disabled = locked || discovering;
    $('setup-discover').textContent = t(discovering ? 'Suche läuft …' : 'Installationen suchen');
    let message = discoveryError ? t('Die Installationssuche ist gerade nicht verfügbar. Du kannst einen Ordner selbst auswählen.')
      : discovery === null ? t('Noch nicht nach Installationen gesucht.')
      : discovery.runtimes.length ? plural(discovery.runtimes.length,'{count} Installation gefunden.','{count} Installationen gefunden.',{count:discovery.runtimes.length})
      : t('Keine vorbereitete Installation gefunden. Wähle deinen Installationsordner.');
    if(discovery?.limited) message += ' '+t('Die Suche war begrenzt. Weitere Ordner kannst du selbst auswählen.');
    $('discovery-message').textContent=message;
    for(const row of candidateRows) {
      row.input.disabled=locked;row.input.checked=row.path===$('runtime-path').value;
      row.badge.textContent=t(row.configured?'Verbunden':row.ready?'Bereit':'Prüfen');
    }
  }
  async function discover() {
    if(discovering || pending || setupBusy(data?.job) || !isOnline())return;
    discovering=true;discoveryError=false;render();
    try {
      const result=await request('/api/setup/discover');
      if(!result || !Array.isArray(result.runtimes))throw new Error('Invalid discovery response');
      discovery={runtimes:result.runtimes.slice(0,40).filter(item=>item && typeof item.path==='string' && item.path.startsWith('/') && item.path.length<=2048),limited:result.limited===true};
      candidateRows.length=0;$('discovery-results').replaceChildren();
      for(const item of discovery.runtimes) {
        const label=document.createElement('label');label.className='discovery-choice';
        const input=document.createElement('input');input.type='radio';input.name='runtime_candidate';input.value=item.path;
        const content=document.createElement('span'),name=document.createElement('strong'),path=document.createElement('small');
        name.textContent=stringValue(item.name,'Microsoft Flight Simulator 2024',180);path.textContent=item.path;content.append(name,path);
        const badge=document.createElement('span');badge.className='discovery-badge';
        input.addEventListener('change',()=>{if(input.checked){$('runtime-path').value=item.path;$('setup-manual').open=false;render();}});
        label.append(input,content,badge);$('discovery-results').append(label);candidateRows.push({input,badge,path:item.path,ready:item.ready===true,configured:item.configured===true});
      }
      $('setup-manual').open=!discovery.runtimes.length;
    } catch { discoveryError=true;$('setup-manual').open=true; }
    finally { discovering=false;render(); }
  }
  function advanceExisting() {
    const job=data?.job;
    if(!connectAfterCheck || !ownCheckId || job?.id!==ownCheckId || job.state!=='ready' || pending || !isOnline() || getStatus()?.game.state!=='stopped')return;
    // The explicit Connect action authorizes only this exact existing-runtime
    // check. Discovery and unrelated/previous jobs can never trigger a commit.
    connectAfterCheck=false;ownCheckId=null;
    void mutate('/api/setup/start',{check_id:job.id});
  }
  const modes = [...document.querySelectorAll('input[name="setup_mode"]')];
  const pickerButtons=[];
  for(const [field,id] of Object.entries(fieldIds)) {
    if(field==='market')continue;
    const input=$(id), row=document.createElement('div');row.className='path-picker';
    input.before(row);row.append(input);
    const button=document.createElement('button');button.type='button';button.className='button secondary';
    button.dataset.input=id;button.hidden=true;
    button.addEventListener('click',()=>void pick(field,id));row.append(button);pickerButtons.push(button);
  }
  const installDestinationButton=$('install-destination-pick');
  installDestinationButton.addEventListener('click',()=>void pick('destination_path','install-destination'));
  pickerButtons.push(installDestinationButton);
  function render() {
    renderRegions();
    const job = data?.job, busy = setupBusy(job), allowed = !!data?.available && online && isOnline() && getStatus()?.game.state === 'stopped';
    const locked = pending || busy || !allowed;
    modes.forEach(input => {input.checked = input.value === mode; input.disabled = locked || (input.value === 'prepare' && !data?.prepare_available);});
    $('setup-install-fields').hidden=mode!=='install';
    $('setup-advanced').hidden=mode==='install';
    $('setup-form-detail').hidden=mode==='install';
    const defaultDestination=stringValue(data?.defaults?.destination_path).replace(/\/(msfs2020|msfs2024)$/,`/${$('setup-game-id').value}`);
    $('install-destination-summary').textContent=$('install-destination').value.trim()||defaultDestination||t('Wird beim Prüfen festgelegt.');
    const destinationHelp=t('Wähle einen vorhandenen übergeordneten Ordner. Flightdeck legt darin {name} neu an.',{name:$('setup-game-id').value});
    $('install-destination-help').textContent=destinationHelp;
    $('setup-destination-help').textContent=destinationHelp;
    $('setup-version-field').hidden=mode==='existing';
    $('setup-game-id').disabled=locked||mode==='existing';
    $('setup-game').placeholder=t('/pfad/zu/MSFS2024').replace('MSFS2024',$('setup-game-id').value==='msfs2020'?'MSFS2020':'MSFS2024');
    for(const id of ['install-market','install-destination'])$(id).disabled=locked||mode!=='install';
    $('install-market').required=mode==='install';
    $('install-unavailable').hidden=data?.install_available===true;
    $('install-unavailable').textContent=data?.install_unavailable_reason||t('Automatische Installation ist in dieser Version nicht verfügbar. Du kannst eine vorhandene Installation verbinden.');
    $('setup-existing-fields').hidden = mode !== 'existing'; $('setup-prepare-fields').hidden = mode !== 'prepare';
    $('setup-form-title').textContent = mode==='install'?t('MSFS installieren'):mode === 'existing' ? t('Deine Installation') : t('Deine neue Runtime');
    $('setup-form-detail').textContent = mode==='install'?t('Mit deinem Microsoft-Konto anmelden und deine gekaufte Xbox-PC-Version herunterladen.'):mode === 'existing' ? t('Wähle den Ordner, in dem deine vorbereitete Runtime liegt.') : t('Führe deine vorhandenen Komponenten in einem neuen lokalen Ordner zusammen.');
    for (const id of Object.values(fieldIds)) $(id).disabled = locked;
    $('setup-market').disabled=locked||mode!=='prepare';$('setup-market').required=mode==='prepare';
    pickerButtons.forEach(button=>{
      const destination=button.id==='install-destination-pick';
      button.textContent=t(destination?'Ordner wählen …':'Durchsuchen …');
      button.setAttribute('aria-label',destination?t('Übergeordneten Speicherort wählen'):t('{field} auswählen',{field:$(button.dataset.input).labels[0]?.textContent||t('Ordner')}));
      button.hidden=!data?.directory_picker;button.disabled=locked;
    });
    renderDiscovery(locked);
    $('config-button').disabled = locked || (mode==='install'&&(!data?.install_available||!normalizeRegion($('install-market').value))) || (mode==='prepare'&&!normalizeRegion($('setup-market').value)) || (mode==='existing'&&!$('runtime-path').value.trim());
    $('setup-build-help').hidden=mode==='install'||!$('setup-advanced').open;
    $('config-button').lastChild.textContent = pending ? t(' Bitte warten …') : mode==='install'?t('MSFS installieren'):mode==='existing' ? t('Verbinden') : t('Installation prüfen');
    const unavailable = errorMessage || (!data?.available && data ? t('Einrichtung ist in diesem Launcher nicht verfügbar.') : '') ||
      (!online ? t('Einrichtungsdienst wird kontaktiert.') : '') ||
      (mode==='prepare'&&!data?.prepare_available && data?.prepare_unavailable_reason ? data.prepare_unavailable_reason : '');
    $('setup-unavailable').textContent = unavailable; $('setup-unavailable').hidden = !unavailable;
    $('runtime-form').hidden = !!job;
    $('setup-mode-choices').hidden=!!job;
    $('setup-form-title').hidden=!!job;
    const updateJob=job?.mode==='update';
    $('setup-job').hidden = !job||updateJob;
    $('setup-update-link').hidden=!updateJob;
    $('setup-steps').hidden=updateJob;
    if(updateJob)$('setup-form-detail').hidden=true;
    const step = !job ? 1 : mode==='install'
      ? job.state==='ready'||job.phase==='authentication'||job.failure_phase==='authentication'?2
        : job.state==='checking'||job.phase==='bootstrap'||(job.state==='failed'&&!['download','provision'].includes(job.failure_phase))?1:3
      : ['checking', 'ready', 'failed', 'cancelled'].includes(job.state)?2:3;
    const steps=mode==='install'?['Voraussetzungen prüfen','Anmelden','Installieren']:['Installation wählen','Prüfen','Verbinden'];
    steps.forEach((label,i)=>{$('setup-step-'+(i+1)).textContent=t(label);});
    for (let i = 1; i <= 3; ++i) {const el=$('setup-step-'+i);el.className=i===step?'active':i<step?'done':'';if(i===step)el.setAttribute('aria-current','step');else el.removeAttribute('aria-current');}
    if (job) {
      const target=(mode==='install'||mode==='prepare')&&['ready','complete'].includes(job.state)?job.runtime_path:'';
      $('setup-target').hidden=!target;
      $('setup-target-label').textContent=target?t('Neuer Speicherort für {game}:',{game:job.game_id==='msfs2020'?'MSFS 2020':'MSFS 2024'}):'';
      $('setup-target-path').textContent=target;
      const titles={checking:t('Installation wird geprüft'),ready:t('Alles geprüft. Bereit zum Einrichten.'),installing:t('Deine Runtime wird eingerichtet'),complete:t('Deine Runtime ist verbunden.'),failed:t('Einrichtung nicht abgeschlossen'),cancelled:t('Einrichtung abgebrochen')};
      $('setup-job-title').textContent=mode==='install'&&job.state==='installing'?t(({bootstrap:'Komponenten werden vorbereitet',authentication:'Microsoft-Anmeldung',download:'MSFS wird heruntergeladen',pausing:'Download wird pausiert',paused:'Download pausiert',provision:'Installation wird eingerichtet'})[job.phase]||'Installation wird eingerichtet'):titles[job.state]; $('setup-job-message').textContent=job.message;
      $('setup-job-message').hidden=!!job.error&&job.message===job.error;
      const help=installationHelp(job);$('setup-next-step').textContent=help;$('setup-next-step').hidden=!help;
      $('setup-install-help').hidden=mode!=='install'||job.state!=='failed';
      $('setup-error').textContent=job.error; $('setup-error').hidden=!job.error;
      renderJobProgress($('setup-progress'), $('setup-transfer'), job);
      renderChecks('setup-checks',job.checks,t('Prüfergebnisse werden vom lokalen Dienst ermittelt.'));
      $('setup-check-details').hidden=!job.checks.length;
      $('setup-start').hidden=job.state!=='ready'; $('setup-start').disabled=pending||!allowed;
      $('setup-start').textContent=mode==='install'?t('Anmelden & herunterladen'):mode==='existing'?t('Runtime verbinden'):t('Runtime einrichten');
      $('setup-cancel').hidden=!busy; $('setup-cancel').disabled=pending||!online||!isOnline();
      $('setup-cancel').textContent=job.state==='ready'?t('Zurück zu den Angaben'):t('Abbrechen');
      const controls=downloadActions(job);
      $('setup-pause').hidden=!controls.pause&&job.phase!=='pausing';$('setup-pause').disabled=!controls.pause||pending||!allowed;
      $('setup-pause').textContent=t(job.phase==='pausing'?'Wird pausiert …':'Download pausieren');
      $('setup-resume').hidden=!controls.resume;$('setup-resume').disabled=!controls.resume||pending||!allowed;
      $('setup-reset').hidden=!['failed','cancelled'].includes(job.state); $('setup-reset').disabled=pending;
      $('setup-reset').textContent=t(mode==='install'&&job.state==='failed'?'Angaben prüfen & erneut versuchen':'Angaben bearbeiten');
      $('setup-complete').hidden=job.state!=='complete';
    }
    changed(busy||pending);
  }
  function apply(raw) {
    const next=normalizeSetup(raw);
    // Ready/terminal updates stay in their own view without reserving setup.
    if(next.job?.mode==='update'&&!setupBusy(next.job))next.job=null;
    if(!initialized){
      const defaults=next.defaults;
      mode=defaults.mode==='prepare'&&next.prepare_available?'prepare':defaults.mode==='install'||!defaults.runtime_path?'install':'existing';
      for(const [key,id] of Object.entries(fieldIds)) $(id).value=stringValue(defaults[key]);
      if(!$('runtime-path').value)$('runtime-path').value=getStatus()?.runtime.path||'';
      $('install-market').value=normalizeRegion(defaults.market);$('setup-market').value=normalizeRegion(defaults.market);
      $('setup-game-id').value=['msfs2020','msfs2024'].includes(defaults.game_id)?defaults.game_id:'msfs2024';
      suggestedDestination=stringValue(defaults.destination_path);
      $('install-destination').value=suggestedDestination;initialized=true;
      applyPreferredSetup();
    }
    if(next.job?.id===ignoredId&&!setupBusy(next.job))next.job=null;
    if(next.job?.mode&&next.job.mode!=='update') {
      mode=next.job.mode;
      if(mode==='install'||mode==='prepare')$(mode==='install'?'install-market':'setup-market').value=next.job.market;
      if(mode==='install'||mode==='prepare')$('setup-game-id').value=next.job.game_id;
    }
    data=next;online=true;errorMessage='';render();advanceExisting();
    if(data.job?.state==='complete'&&completedId!==data.job.id){completedId=data.job.id;void refreshStatus();}
  }
  async function poll() {
    if(polling)return polling;
    polling=(async()=>{try{apply(await request('/api/setup'));if(discovery===null&&!discoveryError&&location.hash==='#installation')void discover();}catch(error){online=false;errorMessage=error.message;render();}finally{polling=null;}})();
    return polling;
  }
  async function pick(field,id) {
    if(pending||setupBusy(data?.job)||!data?.directory_picker||!isOnline()||!getStatus()?.csrf_token)return;
    pending=true;render();
    try {
      const destination=field==='destination_path';
      const value=$(id).value.trim().replace(/\/+$/,'');
      const initial=destination&&value.startsWith('/')?value.slice(0,value.lastIndexOf('/'))||'/':value;
      const result=await request('/api/setup/pick',{method:'POST',body:{field,...(initial.startsWith('/')?{initial}:{})},token:getStatus().csrf_token,timeout:130000});
      if(result.cancelled===false&&result.field===field&&typeof result.path==='string'&&result.path.startsWith('/')){
        const parent=result.path.replace(/\/+$/,'')||'/';
        const selected=destination?(parent==='/'?'':parent)+'/'+$('setup-game-id').value:result.path;
        $(id).value=selected;
        if(destination)suggestedDestination=selected;
      }
    }catch(error){notice(error.message,true);}
    finally{pending=false;render();}
  }
  async function mutate(path,body) {
    if(pending||!isOnline()||!getStatus()?.csrf_token)return;
    pending=true;errorMessage='';render();
    try{
      const result=await request(path,{method:'POST',body,token:getStatus().csrf_token});
      if(result.job&&data){
        data={...data,job:normalizeSetup({...data,job:result.job}).job};
        if(path==='/api/setup/check'&&connectAfterCheck)ownCheckId=data.job.id;
        if(['failed','cancelled','complete'].includes(data.job.state)){connectAfterCheck=false;ownCheckId=null;}
        render();
      }
    }catch(error){connectAfterCheck=false;ownCheckId=null;errorMessage=error.message;notice(error.message,true);}
    finally{if(polling)await polling;await poll();await refreshStatus();pending=false;render();advanceExisting();}
  }
  modes.forEach(input=>input.addEventListener('change',()=>{if(!pending&&!setupBusy(data?.job)){mode=input.value;render();}}));
  $('runtime-form').addEventListener('submit',event=>{
    event.preventDefault();if(!online||pending||setupBusy(data?.job)||getStatus()?.game.state!=='stopped')return;
    const body={mode};
    if(mode==='install'){if(!data?.install_available)return;body.market=$('install-market').value.trim().toUpperCase();body.local_saves=true;body.game_id=$('setup-game-id').value;if($('install-destination').value.trim())body.destination_path=$('install-destination').value;}
    else if(mode==='existing')body.runtime_path=$('runtime-path').value;
    else {for(const [key,id] of Object.entries(fieldIds))if(key!=='runtime_path')body[key]=$(id).value;body.local_saves=true;body.game_id=$('setup-game-id').value;}
    const required=mode==='install'?['market']:mode==='existing'?['runtime_path']:['artifacts_path','game_path','runner_path','prefix_path','destination_path','market'];
    if(mode!=='existing'&&!normalizeRegion(body.market)){notice(t('Bitte wähle deine Store-Region aus.'),true);$(mode==='install'?'install-market':'setup-market').focus();return;}
    if(required.some(key=>!body[key]?.trim())){notice(t('Bitte fülle alle benötigten Pfade aus und wähle eine Store-Region.'),true);return;}
    ignoredId=null;ownCheckId=null;connectAfterCheck=mode==='existing';void mutate('/api/setup/check',body);
  });
  $('setup-start').addEventListener('click',()=>{if(data?.job?.mode!=='update'&&data?.job?.state==='ready'&&!pending)void mutate('/api/setup/start',{check_id:data.job.id});});
  $('setup-cancel').addEventListener('click',()=>{if(setupBusy(data?.job)&&!pending){connectAfterCheck=false;ownCheckId=null;void mutate('/api/setup/cancel',{job_id:data.job.id});}});
  $('setup-pause').addEventListener('click',()=>{if(downloadActions(data?.job).pause&&!pending)void mutate('/api/setup/pause',{job_id:data.job.id});});
  $('setup-resume').addEventListener('click',()=>{if(downloadActions(data?.job).resume&&!pending)void mutate('/api/setup/resume',{job_id:data.job.id});});
  $('setup-reset').addEventListener('click',()=>{ignoredId=data?.job?.id;if(data)data={...data,job:null};errorMessage='';render();});
  $('setup-discover').addEventListener('click',()=>void discover());
  $('runtime-path').addEventListener('input',()=>render());
  $('install-destination').addEventListener('input',()=>render());
  $('setup-game-id').addEventListener('change',()=>{setEdition($('setup-game-id').value);render();});
  for(const id of ['install-market','setup-market'])$(id).addEventListener('change',()=>render());
  $('setup-advanced').addEventListener('toggle',()=>{
    if(!$('setup-advanced').open&&mode==='prepare'&&!pending&&!setupBusy(data?.job))mode=getStatus()?.runtime.configured?'existing':'install';
    render();
  });
  setInterval(()=>{if(!document.hidden&&!pending&&(location.hash==='#installation'||setupBusy(data?.job)))void poll();},1500);
  void poll();
  return {poll,render,discover,job:()=>data?.job,reserved:()=>pending||setupBusy(data?.job),
    chooseInstall:gameId=>focusSetup({mode:'install',gameId}),
    chooseExisting:path=>focusSetup({mode:'existing',path})};
}
