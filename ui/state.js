import {t, locale} from './i18n.js';
// Keep policy separate from rendering so missing/stale status cannot enable actions.
export const VIEWS = Object.freeze({
  overview: 'Übersicht', installation: 'Einrichtung', updates: 'Updates', saves: 'Spielstände', mods: 'Mods', diagnostics: 'Diagnose',
});

export function stringValue(value, fallback = '', limit = 2048) {
  return typeof value === 'string' ? value.slice(0, limit) : fallback;
}

// Automatic actions require an explicit current server request, never an inferred job.
export function normalizeAutomatic(raw) {
  if(raw == null)return null;
  if(typeof raw.enabled!=='boolean'||!['idle','syncing','playing','synced','attention','local'].includes(raw.state)||
    ![null,'before_start','after_exit'].includes(raw.phase)||
    (raw.request_id!==null&&(typeof raw.request_id!=='string'||! /^(?:[a-f0-9]{32}|[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12})$/i.test(raw.request_id))))return null;
  const summary={};
  for(const key of ['container_count','local_container_count','conflict_count']) {
    const value=raw.summary?.[key];if(Number.isSafeInteger(value)&&value>=0)summary[key]=value;
  }
  return {enabled:raw.enabled,state:raw.state,phase:raw.phase,request_id:raw.request_id,
    message:stringValue(raw.message,'',1600),error_code:stringValue(raw.error_code,'',100)||null,
    can_retry:raw.can_retry===true,can_play_local:raw.can_play_local===true,can_cancel:raw.can_cancel===true,
    conflict:raw.conflict===true,summary,last_synced_at:typeof raw.last_synced_at==='string'?raw.last_synced_at.slice(0,100):null};
}
export const automaticBusy=status=>['syncing','playing','attention'].includes(status?.cloud?.state);

export function normalizeStatus(raw) {
  if (!raw || typeof raw !== 'object' || !raw.runtime || !raw.game || !raw.saves) {
    throw new Error(t('Der lokale Dienst hat einen unvollständigen Status geliefert.'));
  }
  const allowedStates = ['stopped', 'starting', 'running', 'stopping', 'external'];
  const state = allowedStates.includes(raw.game.state) ? raw.game.state : 'unknown';
  return {
    app: {name: stringValue(raw.app?.name, 'Flightdeck'), version: stringValue(raw.app?.version, '', 80)},
    service: {update_pending: raw.service?.update_pending === true,
      message: stringValue(raw.service?.message, '', 1000)},
    runtime: {
      configured: raw.runtime.configured === true, path: stringValue(raw.runtime.path),
      ready: raw.runtime.ready === true, checks: normalizeChecks(raw.runtime.checks),
    },
    game: {
      state, managed: raw.game.managed === true,
      can_start: raw.game.can_start === true && state === 'stopped' && raw.runtime.ready === true,
      can_stop: raw.game.can_stop === true && raw.game.managed === true && ['starting', 'running'].includes(state),
      started_at: raw.game.started_at,
      exit_code: Number.isInteger(raw.game.exit_code) ? raw.game.exit_code : null,
    },
    saves: {
      mode: raw.saves.mode === 'local' ? 'local' : 'unavailable',
      available: raw.saves.available === true,
      can_backup: raw.saves.can_backup === true && raw.saves.available === true && raw.saves.mode === 'local',
      bytes: nonnegativeNumber(raw.saves.bytes), files: nonnegativeNumber(raw.saves.files),
      backups: nonnegativeNumber(raw.saves.backups), last_backup: raw.saves.last_backup,
    },
    cloud: normalizeAutomatic(raw.cloud),
    csrf_token: stringValue(raw.csrf_token, '', 512),
  };
}

export function normalizeChecks(checks) {
  if (!Array.isArray(checks)) return [];
  return checks.slice(0, 100).map(check => ({
    label: stringValue(check?.label, t('Prüfung'), 180),
    detail: stringValue(check?.detail, '', 1000),
    ok: check?.ok === true ? true : check?.ok === false ? false : null,
  }));
}

export function nonnegativeNumber(value) {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : null;
}

export function formatCount(value) {
  return nonnegativeNumber(value) === null ? '—' : new Intl.NumberFormat(locale()).format(value);
}

export function formatBytes(value) {
  if (nonnegativeNumber(value) === null) return '—';
  if (value < 1024) return `${formatCount(value)} B`;
  const unit = Math.min(Math.floor(Math.log(value) / Math.log(1024)), 4);
  return `${new Intl.NumberFormat(locale(), {maximumFractionDigits: 1}).format(value / 1024 ** unit)} ${['B', 'KiB', 'MiB', 'GiB', 'TiB'][unit]}`;
}

export function normalizeTransfer(raw) {
  const integer = value => Number.isSafeInteger(value) && value >= 0;
  const optional = (value, positive = false) => value == null || (integer(value) && (!positive || value > 0));
  if (!raw || !['game', 'components'].includes(raw.kind) ||
      !integer(raw.received_bytes) || !integer(raw.verified_bytes) ||
      raw.verified_bytes > raw.received_bytes || !optional(raw.total_bytes, true) ||
      (raw.total_bytes != null && raw.received_bytes > raw.total_bytes) ||
      !optional(raw.completed_files) || !optional(raw.total_files, true) ||
      (raw.completed_files != null && raw.total_files != null && raw.completed_files > raw.total_files)) return null;
  return {kind:raw.kind, received_bytes:raw.received_bytes, verified_bytes:raw.verified_bytes,
    total_bytes:raw.total_bytes ?? null, completed_files:raw.completed_files ?? null, total_files:raw.total_files ?? null};
}

function transferBytes(value) {
  const unit = value < 1000 ? 0 : Math.min(Math.floor(Math.log10(value) / 3), 4);
  return `${new Intl.NumberFormat(locale(), {maximumFractionDigits:1}).format(value / 1000 ** unit)} ${['B','KB','MB','GB','TB'][unit]}`;
}

export function jobProgress(job) {
  const active = !!job && ['checking','installing'].includes(job.state);
  const download = active && ['bootstrap','download','pausing','paused'].includes(job.phase);
  const transfer = download ? normalizeTransfer(job.transfer) : null;
  const matches = transfer && (job.phase === 'bootstrap' ? transfer.kind === 'components' : transfer.kind === 'game');
  let value = download ? null : job?.progress ?? null, detail = '';
  if (matches) {
    if (transfer.total_bytes !== null) {
      // Never round an unfinished or not-yet-verified transfer up to 100%.
      const filesComplete = transfer.completed_files === null || transfer.total_files === null || transfer.completed_files === transfer.total_files;
      value = transfer.received_bytes === transfer.total_bytes && transfer.verified_bytes === transfer.total_bytes && filesComplete
        ? 100 : Math.min(99.9, Math.floor(transfer.received_bytes / transfer.total_bytes * 1000) / 10);
      detail = t('{received} von {total} · {percent} %', {received:transferBytes(transfer.received_bytes),
        total:transferBytes(transfer.total_bytes), percent:new Intl.NumberFormat(locale(), {maximumFractionDigits:1}).format(value)});
    } else detail = t('{received} empfangen', {received:transferBytes(transfer.received_bytes)});
    if (job.phase === 'paused') detail = t('Pausiert · {progress}', {progress:detail});
  }
  // A paused unknown-size transfer has text, but no moving indeterminate bar.
  return {visible:active && (job.phase !== 'paused' || (matches && value !== null)), value, detail};
}

export function renderJobProgress(bar, label, job) {
  const {visible, value, detail} = jobProgress(job);
  bar.hidden = !visible;
  if (value === null) { if (bar.hasAttribute('value')) bar.removeAttribute('value'); }
  else if (bar.getAttribute('value') !== String(value)) bar.value = value;
  label.hidden = !detail;
  if (label.textContent !== detail) label.textContent = detail;
}

export function formatDate(value) {
  if ((typeof value !== 'string' || !value) && typeof value !== 'number') return null;
  const date = new Date(typeof value === 'number' ? value * 1000 : value);
  if (!Number.isFinite(date.getTime())) return null;
  return new Intl.DateTimeFormat(locale(), {dateStyle: 'medium', timeStyle: 'short'}).format(date);
}

export function gamePresentation(status, online = true) {
  if (!online) return {label: t('Lokaler Dienst nicht erreichbar'), detail: t('Die Verbindung wird automatisch erneut geprüft.'), action: t('Simulator starten'), icon: 'play'};
  if (!status) return {label: t('Status wird geladen'), detail: t('Lokaler Dienst wird kontaktiert.'), action: t('Simulator starten'), icon: 'play'};
  const game = status.game;
  const map = {
    starting: {label: t('Simulator startet'), detail: t('Die Initialisierung kann einige Minuten dauern.'), action: t('Start abbrechen'), icon: 'stop'},
    running: {label: t('Simulator läuft'), detail: t('Der Simulator ist in seinem eigenen Fenster geöffnet.'), action: t('Simulator beenden'), icon: 'stop'},
    stopping: {label: t('Simulator wird beendet'), detail: t('Warte, bis der Prozess vollständig beendet ist.'), action: t('Wird beendet …'), icon: 'stop'},
    external: {label: t('Simulator läuft außerhalb von Flightdeck'), detail: t('Diese Sitzung wird hier nicht verwaltet.'), action: t('Extern gestartet'), icon: 'play'},
    unknown: {label: t('Spielstatus nicht verfügbar'), detail: t('Der Dienst meldet keinen bekannten Spielzustand.'), action: t('Simulator starten'), icon: 'play'},
  };
  if (map[game.state]) return map[game.state];
  if(status.cloud?.state==='syncing')return {label:t('Spielstände werden synchronisiert'),detail:status.cloud.message,action:t(status.cloud.phase==='after_exit'?'Spielstände werden gesichert …':'Start wird vorbereitet …'),icon:'play'};
  if(status.cloud?.state==='attention')return {label:t(status.cloud.conflict?'Spielstand auswählen':'Cloud-Abgleich braucht Aufmerksamkeit'),detail:status.cloud.message,action:t('Auswahl erforderlich'),icon:'play'};
  if (!status.runtime.configured) return {label: t('Installation noch nicht verbunden'), detail: t('Installiere MSFS mit deinem Microsoft-Konto oder verbinde eine vorhandene Installation.'), action: t('Installation einrichten'), icon: 'folder'};
  if (!status.runtime.ready) return {label: t('Installation benötigt Aufmerksamkeit'), detail: t('Prüfe die Voraussetzungen weiter unten.'), action: t('Installation prüfen'), icon: 'folder'};
  return {label: game.can_start ? t('Bereit zum Start') : t('Start derzeit nicht verfügbar'), detail: t(status.cloud?.enabled?'Deine Spielstände werden vor dem Start automatisch abgeglichen.':'Anmeldung und lokale Dienste starten mit dem Simulator.'), action: t('Simulator starten'), icon: 'play'};
}

export function actionPermissions(status, online, pending) {
  const connected = !!status && online && !pending && !!status.csrf_token;
  return {
    start: connected && status.game.can_start && !automaticBusy(status),
    stop: connected && status.game.can_stop,
    configure: connected && status.game.state === 'stopped' && !automaticBusy(status),
    backup: connected && status.saves.can_backup && !automaticBusy(status),
    setup: !!status && online && !pending && status.game.state === 'stopped' && !status.runtime.ready && !automaticBusy(status),
  };
}

export function diagnosticValue(value) {
  if (value === null || value === undefined) return t('Nicht verfügbar');
  if (typeof value === 'boolean') return value ? t('Ja') : t('Nein');
  if (typeof value === 'string' || typeof value === 'number') return String(value).slice(0, 4096);
  return JSON.stringify(value, null, 2).slice(0, 8192);
}
