import {t, locale, plural, getLanguage, setLanguage, applyTranslations} from './i18n.js';
import {createSetup} from './setup.js';
import {createMods} from './mods.js';
import {createUpdates} from './updates.js';
import {createCloudSaves} from './cloud-saves.js';
import {VIEWS, stringValue, normalizeStatus, normalizeChecks, formatBytes, formatCount, formatDate, gamePresentation, actionPermissions, diagnosticValue, automaticBusy} from './state.js';

const $ = id => document.getElementById(id);
const state = {status: null, online: false, pending: null, view: 'overview', report: null, reportLoading: false};
let statusRequest = null;
let setupReserved = false;
let setupController = null;
let modsController = null;
let updatesController = null;
let updateReserved = false;
let cloudReserved = false;
let cloudController = null;

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
  text('notice', message);
  $('notice').className = `notice${error ? ' error' : ''}`;
  $('notice').hidden = !message;
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
  modsController?.render();
  updatesController?.render();
  cloudController?.render();
  const status = state.status;
  const permissions = actionPermissions(status, state.online, state.pending || setupReserved || updateReserved || cloudReserved);
  const game = gamePresentation(status, state.online);
  const connected = state.online && !!status;
  text('service-notice', status?.service?.message || '');
  $('service-notice').hidden = !connected || !status?.service?.update_pending || !status.service.message;
  $('connection').className = `connection ${connected ? 'online' : 'offline'}`;
  text('connection-label', connected ? t('Lokaler Dienst verbunden') : t('Lokaler Dienst nicht erreichbar'));
  text('game-state', game.label);
  text('launch-detail', game.detail);
  $('game-status-icon').classList.toggle('ready',permissions.start || status?.game.state==='running');
  $('game-status-use').setAttribute('href',permissions.start?'#i-check-circle':status?.game.state==='running'?'#i-pulse':'#i-info');
  $('connection-icon').setAttribute('href',connected?'#i-check-circle':'#i-info');
  $('launch-icon').setAttribute('href', `#i-${game.icon}`);
  text('launch-label', state.pending === 'launch' ? t('Start wird angefordert …') : state.pending === 'stop' ? t('Beenden angefordert …') : game.action);
  $('launch-button').disabled = !(permissions.start || permissions.stop || permissions.setup);
  $('backup-button').disabled = !permissions.backup;
  text('backup-label', state.pending === 'backup' ? t('Backup wird erstellt …') : t('Backup erstellen'));
  $('refresh-status').disabled = !!state.pending;
  if (!status) return;

  text('app-version', status.app.version ? `${status.app.name} ${status.app.version}` : t('Flightdeck für Linux'));
  const started = formatDate(status.game.started_at);
  text('session-detail', ['running', 'starting', 'stopping'].includes(status.game.state) && started
    ? t('Gestartet {time}', {time:started})
    : status.game.state === 'stopped' && status.game.exit_code !== null ? t('Letzter Exit-Code: {code}', {code:status.game.exit_code}) : t('Lokale Sitzung'));
  renderChecks('overview-checks', status.runtime.checks);
  renderChecks('installation-checks', status.runtime.checks);
  text('current-path', status.runtime.path || t('Noch kein Ordner verbunden.'));

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

async function refreshStatus() {
  if (statusRequest) return statusRequest;
  statusRequest = (async () => {
    try { state.status = normalizeStatus(await request('/api/status')); state.online = true; }
    catch { state.online = false; }
    finally { renderStatus(); setupController?.render(); statusRequest = null; }
  })();
  return statusRequest;
}

async function mutate(action, path, body = {}) {
  if (state.pending || !state.online || !state.status?.csrf_token) return;
  state.pending = action; showNotice(''); renderStatus(); setupController?.render();
  try {
    await request(path, {method: 'POST', body, token: state.status.csrf_token});
    const messages = {launch: t('Start angefordert. Der aktuelle Zustand wird geprüft.'), stop: t('Beenden angefordert. Der aktuelle Zustand wird geprüft.'), backup: t('Das lokale Backup wurde erstellt.'), config: t('Runtime-Pfad gespeichert. Die Installation wird geprüft.')};
    showNotice(messages[action]);
  } catch (error) { showNotice(error.message, true); }
  finally {
    // Complete a previous poll before requesting a status newer than the mutation.
    if (statusRequest) await statusRequest;
    await refreshStatus();
    state.pending = null;
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
  if (state.view === 'installation') void setupController?.poll();
  if (state.view === 'mods') void modsController?.load();
  if (state.view === 'updates') void updatesController?.load();
  if (state.view === 'saves') void cloudController?.load();
}

async function loadDiagnostics() {
  if (state.reportLoading) return;
  state.reportLoading = true;
  $('diagnostic-refresh').disabled = true;
  text('diagnostic-refresh-label', t('Bericht wird geladen …'));
  try {
    const raw = await request('/api/diagnostics');
    if (!raw || typeof raw !== 'object' || !raw.summary || typeof raw.summary !== 'object' || !Array.isArray(raw.checks)) {
      throw new Error(t('Der lokale Dienst hat keinen gültigen Diagnosebericht geliefert.'));
    }
    // Explicit allowlist: never export status, CSRF or unrelated response fields.
    state.report = {summary: raw.summary, checks: raw.checks, generated_at: raw.generated_at ?? null};
    const summaryLabels = {run_found: t('Spielsitzung gefunden'), auth_http: t('Xbox-Anmeldung · HTTP-Status'), local_save_init: t('Lokaler Spielstandspeicher'), exit: t('Letztes Sitzungsende')};
    const rows = Object.entries(state.report.summary).slice(0, 100).map(([key, value]) => {
      const row = document.createElement('div');
      const term = document.createElement('dt'); term.textContent = summaryLabels[key] ?? key;
      const description = document.createElement('dd'); description.textContent = diagnosticValue(value);
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
  const permissions = actionPermissions(state.status, state.online, state.pending || setupReserved || updateReserved || cloudReserved);
  if (permissions.stop) void mutate('stop', '/api/stop');
  else if (permissions.start) void mutate('launch', '/api/launch');
  else if (permissions.setup) location.hash = 'installation';
});
$('refresh-status').addEventListener('click', () => void refreshStatus());
$('backup-button').addEventListener('click', () => {
  if (actionPermissions(state.status, state.online, state.pending || setupReserved || updateReserved || cloudReserved).backup) void mutate('backup', '/api/saves/backup');
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
  showNotice(''); setView(); renderStatus(); setupController?.render();
  void refreshStatus(); void setupController?.poll(); void updatesController?.load();
  if (state.report) void loadDiagnostics();
});
window.addEventListener('hashchange', setView);
document.addEventListener('visibilitychange', () => { if (!document.hidden) void refreshStatus(); });
applyTranslations();
setupController = createSetup({request,getStatus:()=>state.status,isOnline:()=>state.online && !state.pending && !updateReserved && !cloudReserved && !automaticBusy(state.status),renderChecks,notice:showNotice,refreshStatus,changed:reserved=>{setupReserved=reserved;renderStatus();}});
modsController = createMods({request,getStatus:()=>state.status,isOnline:()=>state.online&&!state.pending,isReserved:()=>setupReserved||updateReserved||cloudReserved||automaticBusy(state.status),notice:showNotice});
updatesController = createUpdates({request,getStatus:()=>state.status,isOnline:()=>state.online&&!state.pending,getSetupJob:()=>setupController.job(),isReserved:()=>setupReserved||cloudReserved||automaticBusy(state.status),refreshStatus,refreshSetup:()=>setupController.poll(),renderChecks,changed:value=>{updateReserved=value;setupController?.render();renderStatus();}});
cloudController = createCloudSaves({request,getStatus:()=>state.status,isOnline:()=>state.online&&!state.pending,isReserved:()=>setupReserved||updateReserved,refreshStatus,changed:value=>{cloudReserved=value;setupController?.render();renderStatus();}});
setView();
void refreshStatus().then(()=>{setupController.render();void updatesController.load();});
setInterval(() => { if (!document.hidden && !state.pending) void refreshStatus(); }, 3000);
