import {t, locale, plural, getLanguage, setLanguage, applyTranslations} from './i18n.js';
import {createMaintenance} from './maintenance.js';
import {createStoreCheck} from './store-check.js';
import {createProblemReports} from './problem-reports.js';
import {createSetup} from './setup.js';
import {createMods} from './mods.js';
import {createFenix} from './fenix.js';
import {createUpdates} from './updates.js';
import {createLauncherUpdates} from './launcher-updates.js';
import {createNotices} from './notices.js';
import {createCloudSaves} from './cloud-saves.js';
import {VIEWS, stringValue, normalizeStatus, normalizeChecks, formatBytes, formatCount, formatDate, gamePresentation, actionPermissions, diagnosticValue, automaticBusy, graphicsEditable} from './state.js';

const $ = id => document.getElementById(id);
const state = {status: null, online: false, pending: null, pendingGame: null, view: 'overview', report: null, reportLoading: false};
let statusRequest = null;
let graphicsDraft = null;
let vrDraft = null;
let storeCheckController = null;
let problemReportsController = null;
let storeCheckReserved = false;
let maintenanceController = null;
let maintenanceReserved = false;
let setupReserved = false;
let setupController = null;
let modsController = null;
let fenixController = null;
let fenixReserved = false;
let updatesController = null;
let updateReserved = false;
let cloudReserved = false;
let cloudController = null;
let launcherUpdatesController = null;
let launcherUpdateReserved = false;
let startupRequest = null;
const startupChecked = new Set();
const updateOffers = {launcher:false,game:false};

function renderUpdateNotice(kind, available) {
  if(kind)updateOffers[kind]=available;
  $('available-updates').hidden=!updateOffers.launcher&&!updateOffers.game;
  text('available-updates-label',t(updateOffers.launcher&&updateOffers.game?'Updates für Flightdeck und MSFS sind verfügbar.':updateOffers.launcher?'Ein Flightdeck-Update ist verfügbar.':'Ein MSFS-Update ist verfügbar.'));
}

async function checkStartupUpdates() {
  if(startupRequest||!state.online||!state.status?.csrf_token||state.pending)return;
  const idle=state.status.game.state==='stopped'&&!setupReserved&&!fenixReserved&&!updateReserved&&!cloudReserved&&!launcherUpdateReserved&&!maintenanceReserved&&!storeCheckReserved&&!automaticBusy(state.status);
  const key=idle&&state.status.runtime.path?state.status.runtime.path:'launcher';
  if(startupChecked.has(key))return;
  startupChecked.add(key);
  startupRequest=(async()=>{
    try {
      const result=await request('/api/updates/check-startup',{method:'POST',token:state.status.csrf_token});
      if(result.deferred&&key!=='launcher')startupChecked.delete(key);
    } catch { /* Manual checks remain available after an offline startup. */ }
    finally {
      await Promise.all([updatesController?.load({background:true}),launcherUpdatesController?.load()]);
      startupRequest=null;
    }
  })();
  return startupRequest;
}
const notices = createNotices($('notice'));
// Decode both scenes before the first switch. Keep the decoded images alive;
// an opacity-zero CSS background alone may defer decoding until it is shown.
const gameArtwork = ['flight-panorama.png', 'flight-panorama-2020.png'].map(source => {
  const image = new Image(); image.src = new URL(source, import.meta.url); return image;
});
const artworkReady = Promise.all(gameArtwork.map(image => image.decode().catch(() => {})));

function renderGameTheme(gameId) {
  const previous = document.body.dataset.game;
  if (previous === gameId) return;
  document.body.dataset.game = gameId;
  // Paint the initial selection before enabling transitions. Status polls
  // keep the existing layers and focus; only a changed game animates.
  if (!document.body.classList.contains('game-theme-ready')) {
    requestAnimationFrame(() => requestAnimationFrame(() => document.body.classList.add('game-theme-ready')));
  }
}

function text(id, value) { if ($(id).textContent !== value) $(id).textContent = value; }

async function request(path, {method = 'GET', body, token, timeout} = {}) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeout ?? (method === 'GET' ? 10000 : 60000));
  try {
    const requestedLanguage = getLanguage();
    const headers = {Accept: 'application/json', 'Accept-Language': requestedLanguage};
    if (method === 'POST') {
      headers['Content-Type'] = 'application/json';
      headers['X-Flightdeck-Token'] = token;
    }
    const response = await fetch(path, {
      method, headers, cache: 'no-store', credentials: 'same-origin', signal: controller.signal,
      ...(method === 'POST' ? {body: JSON.stringify(body ?? {})} : {}),
    });
    let result;
    try { result = await response.json(); }
    catch { throw new Error(t('Der lokale Dienst hat keine gültige Antwort geliefert.')); }
    // Refetch stale reads in the chosen language; never replay a mutation.
    if (method === 'GET' && requestedLanguage !== getLanguage()) return request(path, {method, timeout});
    if (!response.ok || result?.ok === false) {
      throw new Error(stringValue(result?.error, t('Der lokale Dienst meldet einen Fehler ({status}).', {status:response.status}), 1000));
    }
    return result;
  } catch (error) {
    if (error.name === 'AbortError') throw new Error(t('Der lokale Dienst antwortet nicht rechtzeitig. Bitte prüfe den Status.'));
    if (error instanceof TypeError) throw new Error(t('Der lokale Dienst ist nicht erreichbar.'));
    throw error;
  } finally { clearTimeout(timer); }
}

function showNotice(message, error = false) {
  notices.show(message,error);
}

function renderChecks(target, checks, empty = t('Noch keine Prüfergebnisse verfügbar.')) {
  const items = checks.map(check => {
    const row = document.createElement('li');
    row.className = 'check-row';
    const marker = document.createElement('span');
    marker.className = `check-indicator${check.ok === false ? ' failed' : check.ok === null ? ' unknown' : ''}`;
    marker.setAttribute('aria-hidden', 'true');
    const icon = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    icon.classList.add('icon');
    const use = document.createElementNS('http://www.w3.org/2000/svg', 'use');
    use.setAttribute('href', check.ok === true ? '#i-check' : '#i-info');
    icon.append(use); marker.append(icon);
    const content = document.createElement('div'); content.className = 'check-text';
    const label = document.createElement('strong'); label.textContent = check.label;
    const detail = document.createElement('p'); detail.textContent = check.detail;
    content.append(label, detail);
    const result = document.createElement('span'); result.className = 'check-result';
    result.textContent = check.ok === true ? t('Bereit') : check.ok === false ? t('Prüfen') : t('Unbekannt');
    row.append(marker, content, result);
    return row;
  });
  if (!items.length) {
    const row = document.createElement('li'); row.className = 'empty-state'; row.textContent = empty; items.push(row);
  }
  $(target).replaceChildren(...items);
}

function renderStatus() {
  renderGraphics();
  renderVR();
  problemReportsController?.render();
  document.body.classList.toggle('game-switch-pending', state.pending === 'switch');
  storeCheckController?.render();
  maintenanceController?.render();
  modsController?.render();
  fenixController?.render();
  updatesController?.render();
  launcherUpdatesController?.render();
  cloudController?.render();
  const status = state.status;
  const permissions = actionPermissions(status, state.online, state.pending || fenixReserved || setupReserved || updateReserved || cloudReserved || launcherUpdateReserved || maintenanceReserved || storeCheckReserved);
  const game = gamePresentation(status, state.online);
  const connected = state.online && !!status;
  text('service-notice', status?.service?.message || '');
  $('service-notice').hidden = !connected || !status?.service?.update_pending || !status.service.message;
  $('connection').className = `connection ${connected ? 'online' : 'offline'}`;
  text('connection-label', connected ? t('Lokaler Dienst verbunden') : t('Lokaler Dienst nicht erreichbar'));
  text('game-state', game.label);
  text('launch-detail', game.detail);
  const ready = connected && status.runtime.ready && status.game.state === 'stopped' && status.game.can_start;
  $('game-status-icon').classList.toggle('ready',ready || status?.game.state==='running');
  $('game-status-use').setAttribute('href',ready?'#i-check-circle':status?.game.state==='running'?'#i-pulse':'#i-info');
  $('connection-icon').setAttribute('href',connected?'#i-check-circle':'#i-info');
  $('launch-icon').setAttribute('href', `#i-${game.icon}`);
  text('launch-label', state.pending === 'launch' ? t('Start wird angefordert …') : state.pending === 'stop' ? t('Beenden angefordert …') : game.action);
  $('launch-button').disabled = !(permissions.start || permissions.stop || permissions.setup);
  $('backup-button').disabled = !permissions.backup;
  text('backup-label', state.pending === 'backup' ? t('Backup wird erstellt …') : t('Backup erstellen'));
  $('refresh-status').disabled = !!state.pending;
  const canSwitch = connected && status?.game.state === 'stopped' && !state.pending &&
    !fenixReserved && !setupReserved && !updateReserved && !cloudReserved && !launcherUpdateReserved&&!maintenanceReserved&&!storeCheckReserved && !automaticBusy(status);
  for (const gameId of ['msfs2024','msfs2020']) {
    const button=$('version-'+gameId), item=status?.versions?.[gameId];
    const active=!!status?.runtime.configured && status.runtime.game_id===gameId;
    const selecting=state.pending === 'switch' && state.pendingGame === gameId;
    button.setAttribute('aria-pressed',String(active));
    button.setAttribute('aria-busy',String(selecting));
    button.disabled=!canSwitch||active;
    text('version-'+gameId+'-state',selecting?t('Wechselt …'):!connected?t('Wird geprüft …'):active?t('Aktiv'):item?.ready?t('Wechseln'):item?.installed?t('Einrichtung nötig'):t('Installieren'));
  }
  if (!status) return;

  renderGameTheme(status.runtime.configured ? status.runtime.game_id : '');

  text('app-version', status.app.version ? `${status.app.name} ${status.app.version}` : t('Flightdeck für Linux'));
  const started = formatDate(status.game.started_at);
  text('session-detail', ['running', 'starting', 'stopping'].includes(status.game.state) && started
    ? t('Gestartet {time}', {time:started})
    : status.game.state === 'stopped' && status.game.exit_code !== null ? t('Letzter Exit-Code: {code}', {code:status.game.exit_code}) : t('Lokale Sitzung'));
  renderChecks('overview-checks', status.runtime.checks);
  renderChecks('installation-checks', status.runtime.checks);
  text('current-path', status.runtime.path || t('Noch kein Ordner verbunden.'));
  text('launch-game-name', status.runtime.configured ? status.runtime.game_name : 'Microsoft Flight Simulator');
  text('selected-game-name', status.runtime.configured ? status.runtime.game_name : '');

  const saves = status.saves;
  text('overview-save-value', saves.available ? t('Lokaler Speicher aktiv') : t('Lokaler Speicher nicht verfügbar'));
  text('overview-save-detail', saves.available
    ? t('{bytes} · {files} · {backups}', {bytes:formatBytes(saves.bytes), files:plural(saves.files, '{count} Datei', '{count} Dateien', {count:formatCount(saves.files)}), backups:plural(saves.backups, '{count} Backup', '{count} Backups', {count:formatCount(saves.backups)})})
    : saves.mode === 'local' ? t('Der lokale Speicher ist noch nicht verfügbar.') : t('Lokaler Speicher ist nicht eingerichtet.'));
  text('save-mode-description', saves.mode === 'local'
    ? t(status.cloud?.enabled?'Deine Spielstände werden automatisch abgeglichen. Lokale Sicherungen bleiben auf diesem Rechner.':'Lokale Speicherung ist aktiviert. Backups bleiben auf diesem Rechner.')
    : t('Für diese Runtime ist kein lokaler Spielstandspeicher verfügbar.'));
  text('save-bytes', formatBytes(saves.bytes)); text('save-files', formatCount(saves.files)); text('save-backups', formatCount(saves.backups));
  text('backup-reason', !state.online ? t('Der lokale Dienst ist nicht erreichbar.') : saves.can_backup
    ? t('Der lokale Dienst ist bereit, ein Backup anzulegen.')
    : !saves.available ? t('Lokaler Spielstandspeicher ist nicht verfügbar.')
    : ['starting', 'running', 'stopping', 'external'].includes(status.game.state) ? t('Beende den Simulator, bevor du ein Backup erstellst.')
    : t('Ein Backup ist derzeit nicht verfügbar.'));
  const backup = saves.last_backup;
  text('last-backup', backup === null ? t('Noch kein Backup erstellt.') : formatDate(backup?.created_at) ?? t('Zeitpunkt nicht verfügbar.'));
  text('last-backup-name', stringValue(backup?.name));
}

function canEditGraphics() {
  return graphicsEditable(state.status, state.online, state.pending || setupReserved || fenixReserved ||
    updateReserved || cloudReserved || launcherUpdateReserved || maintenanceReserved || storeCheckReserved);
}

function canEditVR() {
  const s = state.status;
  return !!(s?.runtime.configured && s.vr?.available && state.online && s.csrf_token &&
    s.game.state === 'stopped' && !s.setup.busy && !state.pending && !setupReserved &&
    !fenixReserved && !updateReserved && !cloudReserved && !launcherUpdateReserved &&
    !maintenanceReserved && !storeCheckReserved && !['syncing', 'playing'].includes(s.cloud?.state));
}

function renderVR() {
  const s = state.status;
  $('vr-card').hidden = !s?.runtime.configured || !s.vr?.available;
  if (vrDraft?.path !== s?.runtime.path) vrDraft = null;
  const vr = s?.vr, mode = vrDraft?.mode ?? vr?.mode ?? 'off';
  $('vr-mode').value = mode;
  $('vr-mode').disabled = !canEditVR();
  $('vr-save').disabled = !canEditVR() || (mode === vr?.mode && !vr?.error);
  $('vr-check').disabled = !canEditVR() || mode === 'off' || mode !== vr?.mode;
  text('vr-game', s?.runtime.game_name || '');
  text('vr-status', vr?.error || vr?.message || '');
  $('vr-status').className = vr?.error ? 'notice error' : 'muted';
  const check = vr?.check;
  $('vr-result').hidden = !check;
  text('vr-result', check?.message || '');
  $('vr-result').className = ['ready','checking'].includes(check?.state) ? 'notice' : 'notice error';
  $('vr-checked').hidden = !check || check.state === 'checking';
  text('vr-checked', check ? t('Letzte Prüfung: {time}', {time: formatDate(check.checked_at) || '—'}) : '');
  $('vr-nvidia').hidden = !s?.graphics?.nvidia_present;
}

function renderGraphics() {
  const status = state.status;
  $('graphics-card').hidden = !status?.runtime.configured || !status.graphics?.nvidia_present;
  if (graphicsDraft?.path !== status?.runtime.path) graphicsDraft = null;
  const mode = graphicsDraft?.mode ?? status?.graphics?.nvidia_mode ?? 'auto';
  $('graphics-mode').value = mode;
  $('graphics-mode').disabled = !canEditGraphics();
  $('graphics-save').disabled = !canEditGraphics() || (mode === status?.graphics?.nvidia_mode && !status?.graphics?.error);
  text('graphics-game', status?.runtime.game_name || '');
  text('graphics-description', t(mode !== 'features'
    ? 'Nutzt den NVIDIA-Kompatibilitätsmodus für DirectX 11 und 12. DLSS, Reflex und NVIDIA Frame Generation sind deaktiviert.'
    : 'Nutzt NVIDIA-Funktionen mit den Komponenten des Runners und des installierten Treibers. DLSS benötigt passende Treiberkomponenten.'));
  text('graphics-error', status?.graphics?.error || '');
  $('graphics-error').hidden = !status?.graphics?.error;
  text('graphics-busy', t(status?.graphics?.available
    ? 'Beende zuerst Spiel, Cloud-Abgleich und laufende Einrichtungen.'
    : 'Für diese Installation ist keine NVIDIA-Einrichtung verfügbar.'));
  $('graphics-busy').hidden = canEditGraphics();
}

async function refreshStatus() {
  if (statusRequest) return statusRequest;
  statusRequest = (async () => {
    try { state.status = normalizeStatus(await request('/api/status')); state.online = true; }
    catch { state.online = false; }
    finally { renderStatus(); setupController?.render(); statusRequest = null; maintenanceController?.poll(); storeCheckController?.poll(); void checkStartupUpdates(); }
  })();
  return statusRequest;
}

async function mutate(action, path, body = {}) {
  if (state.pending || !state.online || !state.status?.csrf_token) return;
  state.pending = action;
  state.pendingGame = action === 'switch' ? body.game_id : null;
  text('version-announcement', '');
  showNotice(''); renderStatus(); setupController?.render();
  try {
    await request(path, {method: 'POST', body, token: state.status.csrf_token});
    if (action === 'graphics') graphicsDraft = null;
    if (action === 'vr') vrDraft = null;
    if (action === 'switch') {
      await artworkReady;
      text('version-announcement', t('Simulator gewechselt. Du kannst ihn jetzt starten.'));
    } else {
      const messages = {launch: t('Start angefordert. Der aktuelle Zustand wird geprüft.'), stop: t('Beenden angefordert. Der aktuelle Zustand wird geprüft.'), backup: t('Das lokale Backup wurde erstellt.'), config: t('Runtime-Pfad gespeichert. Die Installation wird geprüft.'), graphics: t('NVIDIA-Modus gespeichert. Er gilt ab dem nächsten Spielstart.')};
      showNotice(action === 'vr' ? t('VR-Modus gespeichert. Er gilt ab dem nächsten Spielstart.') :
        action === 'vrCheck' ? '' : messages[action]);
    }
  } catch (error) { showNotice(error.message, true); }
  finally {
    // Complete a previous poll before requesting a status newer than the mutation.
    if (statusRequest) await statusRequest;
    await refreshStatus();
    state.pending = null;
    state.pendingGame = null;
    renderStatus(); setupController?.render();
  }
}

function setView() {
  const requested = location.hash.slice(1);
  state.view = Object.hasOwn(VIEWS, requested) ? requested : 'overview';
  for (const [view, title] of Object.entries(VIEWS)) {
    $('view-' + view).hidden = view !== state.view;
    const nav = document.querySelector(`[data-nav="${view}"]`);
    if (view === state.view) { nav.setAttribute('aria-current', 'page'); text('page-title', t(title)); }
    else nav.removeAttribute('aria-current');
  }
  document.title = `${t(VIEWS[state.view])} · Flightdeck`;
  if (state.view === 'installation') {void setupController?.poll();void maintenanceController?.load();}
  if (state.view === 'mods') { void modsController?.load(); void fenixController?.load(); }
  if (state.view === 'updates') {void updatesController?.load();void launcherUpdatesController?.load();}
  if (state.view === 'diagnostics') void storeCheckController?.load();
  if (state.view === 'saves') void cloudController?.load();
}

async function loadDiagnostics() {
  if (state.reportLoading) return;
  state.reportLoading = true;
  $('diagnostic-refresh').disabled = true;
  text('diagnostic-refresh-label', t('Bericht wird geladen …'));
  try {
    const raw = await request('/api/diagnostics', {timeout:20000});
    if (!raw || typeof raw !== 'object' || !raw.summary || typeof raw.summary !== 'object' || !Array.isArray(raw.checks)) {
      throw new Error(t('Der lokale Dienst hat keinen gültigen Diagnosebericht geliefert.'));
    }
    // Explicit allowlist: never export status, CSRF or unrelated response fields.
    state.report = {summary: raw.summary, checks: raw.checks, generated_at: raw.generated_at ?? null};
    const summaryLabels = {run_found: t('Spielsitzung gefunden'), auth_http: t('Xbox-Anmeldung · HTTP-Status'), local_save_init: t('Lokaler Spielstandspeicher'), store_calls: t('Store-API-Aufrufe'), store_catalog: t('Marketplace-Abfragen'), store_session:t('Store-Sitzungsverlauf'), store_check:t('Letzte Store-Prüfung'), exit: t('Letztes Sitzungsende'), cloud_sync:t('Xbox-Cloud-Abgleich'), graphics:t('Grafik und Vulkan'), vr:t('Virtual Reality'), audio:t('Audio und Medien'), user_calls:t('Spielanmeldung'), policy_cache:t('Anmelderichtlinien'), signature_policy:t('Anfragesignaturen'), network_security:t('Netzwerksicherheit'), log_coverage:t('Log-Auswertung'), summary_limited:t('Zusammenfassung gekürzt')};
    const rows = Object.entries(state.report.summary).slice(0, 100).map(([key, value]) => {
      const row = document.createElement('div');
      const term = document.createElement('dt'); term.textContent = summaryLabels[key] ?? key;
      const description = document.createElement('dd');
      if (key === 'store_session' && value) {
        description.textContent = t('{count} Store-Ereignisse erfasst.', {count:formatCount(Array.isArray(value.events)?value.events.length:0)}) + '\n' +
          t(value.components_at_launch?'Komponenten beim Spielstart erfasst.':'Komponentenstand dieser Spielsitzung unbekannt.') +
          (value.partial?'\n'+t('Der Verlauf enthält nur einen Ausschnitt.'):'');
      } else if (key === 'store_check' && value) {
        description.textContent = formatDate(value.finished_at||value.started_at) + '\n' +
          t(value.state==='passed'?'Alle Prüfschritte erfolgreich':value.state==='running'?'Store-Prüfung läuft …':value.state==='failed'?'Store-Prüfung mit Fehlern':'Store-Prüfung unvollständig');
      } else description.textContent = diagnosticValue(value);
      row.append(term, description); return row;
    });
    $('diagnostic-summary').replaceChildren(...rows);
    renderChecks('diagnostic-checks', normalizeChecks(raw.checks));
    text('diagnostic-time', t('Erstellt: {time}', {time:formatDate(raw.generated_at) ?? t('Zeitpunkt nicht verfügbar')}));
    text('diagnostic-json', JSON.stringify(state.report, null, 2));
    $('diagnostic-details').hidden = false;
    $('diagnostic-copy').disabled = false; $('diagnostic-download').disabled = false;
  } catch (error) { showNotice(error.message, true); }
  finally {
    state.reportLoading = false; $('diagnostic-refresh').disabled = false;
    text('diagnostic-refresh-label', state.report ? t('Bericht aktualisieren') : t('Bericht laden'));
  }
}

$('launch-button').addEventListener('click', () => {
  const permissions = actionPermissions(state.status, state.online, state.pending || fenixReserved || setupReserved || updateReserved || cloudReserved || launcherUpdateReserved || maintenanceReserved || storeCheckReserved);
  if (permissions.stop) void mutate('stop', '/api/stop');
  else if (permissions.start) void mutate('launch', '/api/launch');
  else if (permissions.setup) location.hash = 'installation';
});
for (const gameId of ['msfs2024','msfs2020']) {
  $('version-'+gameId).addEventListener('click',()=>{
    const status=state.status;
    if (!state.online||!status||state.pending||fenixReserved||setupReserved||updateReserved||cloudReserved||launcherUpdateReserved||maintenanceReserved||storeCheckReserved||automaticBusy(status)||status.game.state!=='stopped')return;
    if (status.runtime.configured&&status.runtime.game_id===gameId)return;
    const version=status.versions[gameId];
    if (version.ready) void mutate('switch','/api/game/select',{game_id:gameId});
    else {
      if (version.installed) setupController?.chooseExisting(version.path);
      else setupController?.chooseInstall(gameId);
      location.hash='installation';
    }
  });
}
$('refresh-status').addEventListener('click', () => void refreshStatus());
$('graphics-mode').addEventListener('change', () => {
  if (canEditGraphics()) graphicsDraft = {path: state.status.runtime.path, mode: $('graphics-mode').value};
  renderGraphics();
});
$('vr-mode').addEventListener('change', () => {
  if (canEditVR()) vrDraft = {path: state.status.runtime.path, mode: $('vr-mode').value};
  renderVR();
});
$('vr-save').addEventListener('click', () => {
  if (canEditVR()) void mutate('vr', '/api/vr/configure', {
    runtime_path: state.status.runtime.path, mode: $('vr-mode').value,
  });
});
$('vr-check').addEventListener('click', () => {
  if (canEditVR() && !$('vr-check').disabled) void mutate('vrCheck', '/api/vr/check', {
    runtime_path: state.status.runtime.path,
  });
});
$('graphics-save').addEventListener('click', () => {
  if (canEditGraphics()) void mutate('graphics', '/api/graphics', {
    runtime_path: state.status.runtime.path, nvidia_mode: $('graphics-mode').value,
  });
});
$('backup-button').addEventListener('click', () => {
  if (actionPermissions(state.status, state.online, state.pending || fenixReserved || setupReserved || updateReserved || cloudReserved || launcherUpdateReserved || maintenanceReserved || storeCheckReserved).backup) void mutate('backup', '/api/saves/backup');
});
$('diagnostic-refresh').addEventListener('click', () => void loadDiagnostics());
$('diagnostic-copy').addEventListener('click', async () => {
  if (!state.report) return;
  try { await navigator.clipboard.writeText(JSON.stringify(state.report, null, 2)); showNotice(t('Der freigegebene Diagnosebericht wurde kopiert.')); }
  catch { showNotice(t('Kopieren ist in diesem Browser nicht verfügbar. Du kannst den Bericht als JSON herunterladen.'), true); }
});
$('diagnostic-download').addEventListener('click', () => {
  if (!state.report) return;
  const url = URL.createObjectURL(new Blob([JSON.stringify(state.report, null, 2) + '\n'], {type: 'application/json'}));
  const link = document.createElement('a'); link.href = url; link.download = 'flightdeck-diagnose.json';
  document.body.append(link); link.click(); link.remove(); setTimeout(() => URL.revokeObjectURL(url), 1000);
});
$('language-select').addEventListener('change', event => setLanguage(event.target.value));
window.addEventListener('flightdeck-languagechange', () => {
  showNotice(''); setView(); renderStatus(); renderUpdateNotice(); setupController?.render();
  void refreshStatus(); void setupController?.poll(); void updatesController?.load();
  void launcherUpdatesController?.load();
  if (state.report) void loadDiagnostics();
});
window.addEventListener('hashchange', setView);
document.addEventListener('visibilitychange', () => { if (!document.hidden) void refreshStatus(); });
applyTranslations();
problemReportsController = createProblemReports({request,getStatus:()=>state.status,isOnline:()=>state.online&&!state.pending});
setupController = createSetup({request,getStatus:()=>state.status,isOnline:()=>state.online && !state.pending && !fenixReserved && !updateReserved && !cloudReserved && !launcherUpdateReserved&&!maintenanceReserved&&!storeCheckReserved && !automaticBusy(state.status),renderChecks,notice:showNotice,refreshStatus,changed:reserved=>{setupReserved=reserved;renderStatus();}});
fenixController = createFenix({request,getStatus:()=>state.status,isOnline:()=>state.online&&!state.pending,isReserved:()=>setupReserved||updateReserved||cloudReserved||launcherUpdateReserved||maintenanceReserved||storeCheckReserved||automaticBusy(state.status),refreshStatus,notice:showNotice,changed:value=>{fenixReserved=value;renderStatus();}});
modsController = createMods({request,getStatus:()=>state.status,isOnline:()=>state.online&&!state.pending,isReserved:()=>fenixReserved||setupReserved||updateReserved||cloudReserved||launcherUpdateReserved||maintenanceReserved||storeCheckReserved||automaticBusy(state.status),notice:showNotice});
updatesController = createUpdates({request,getStatus:()=>state.status,isOnline:()=>state.online&&!state.pending,getSetupJob:()=>setupController.job(),isReserved:()=>fenixReserved||setupReserved||cloudReserved||launcherUpdateReserved||maintenanceReserved||storeCheckReserved||automaticBusy(state.status),refreshStatus,refreshSetup:()=>setupController.poll(),renderChecks,availableChanged:value=>renderUpdateNotice('game',value),changed:value=>{updateReserved=value;setupController?.render();renderStatus();}});
cloudController = createCloudSaves({request,getStatus:()=>state.status,isOnline:()=>state.online&&!state.pending,isReserved:()=>fenixReserved||setupReserved||updateReserved||launcherUpdateReserved||maintenanceReserved||storeCheckReserved,refreshStatus,changed:value=>{cloudReserved=value;setupController?.render();renderStatus();}});
launcherUpdatesController = createLauncherUpdates({request,getStatus:()=>state.status,isOnline:()=>state.online&&!state.pending,
  isReserved:()=>fenixReserved||setupReserved||updateReserved||cloudReserved||maintenanceReserved||storeCheckReserved||automaticBusy(state.status),
  availableChanged:value=>renderUpdateNotice('launcher',value),
  refreshStatus,notice:showNotice,changed:value=>{launcherUpdateReserved=value;setupController?.render();renderStatus();}});
maintenanceController = createMaintenance({request,getStatus:()=>state.status,isOnline:()=>state.online&&!state.pending,
  isSetupActive:()=>setupReserved,
  isReserved:()=>setupReserved||updateReserved||fenixReserved||cloudReserved||launcherUpdateReserved||storeCheckReserved,
  refreshStatus,changed:value=>{maintenanceReserved=value;setupController?.render();renderStatus();}});
storeCheckController = createStoreCheck({request,getStatus:()=>state.status,isOnline:()=>state.online&&!state.pending,
  isReserved:()=>setupReserved||updateReserved||fenixReserved||cloudReserved||launcherUpdateReserved||maintenanceReserved,
  refreshStatus,changed:value=>{storeCheckReserved=value;setupController?.render();renderStatus();}});
setView();
void refreshStatus().then(()=>{setupController.render();void updatesController.load();void launcherUpdatesController.load();});
setInterval(() => { if (!document.hidden && !state.pending) void refreshStatus(); }, 3000);
