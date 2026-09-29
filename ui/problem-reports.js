import {t} from './i18n.js';

const categories = {graphics:'Grafik / NVIDIA', cloud:'Cloud-Sync', marketplace:'Marketplace', installation:'Installation / Start', other:'Anderer Fehler'};

export function reportFilename(report) {
  return `flightdeck-report-${report.id}.txt`;
}

// Plain text retains every collected field. No raw log or credentials are
// collected by the backend. Only the user's own description is free text.
export function reportText(report) {
  const lines = [];
  function fields(value, path = '') {
    if (value && typeof value === 'object') {
      const entries = Object.entries(value);
      if (!entries.length) lines.push(`${path}: —`);
      for (const [key, item] of entries) fields(item, path ? `${path}.${key}` : key);
    } else lines.push(`${path}: ${value ?? '—'}`);
  }
  fields({system:report.system, diagnostics:report.diagnostics});
  return [t('Flightdeck-Fehlerbericht'), `ID: ${report.id}`,
    `${t('Erfasst')}: ${report.created_at}`, `${t('Kategorie')}: ${t(categories[report.category])}`,
    ...(report.observations.length ? [`${t('Beobachtungen')}: ${report.observations.join(', ')}`] : []),
    '', report.description, '', t('Automatisch erfasste Diagnosedaten'), ...lines].join('\n');
}

export function emailDraft(report, recipient) {
  const subject = `Flightdeck · ${report.category} · ${report.id.slice(0, 8)}`;
  const body = reportText(report);
  const valid = typeof recipient === 'string' && /^[A-Za-z0-9.!#$%'+/=_`{|}~-]{1,64}@[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?\.[A-Za-z]{2,63}$/.test(recipient) && recipient.length <= 254 && !recipient.includes('..');
  // RFC 6068: encode field values independently and use CRLF for body lines.
  const uri = valid ? `mailto:${encodeURIComponent(recipient).replace('%40','@')}?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(body.replace(/\r?\n/g, '\r\n'))}` : null;
  // Applications impose different URI limits. Never silently shorten a report:
  // unusually long ones use the complete, visible copy/download fallback.
  const tooLong = Boolean(uri && uri.length > 24000);
  return {subject, body, tooLong, href:tooLong ? null : uri};
}

export function createProblemReports({request, getStatus, isOnline}) {
  const $ = id => document.getElementById(id);
  let draft = null, recipient = null, busy = false, dirty = false, opened = false;
  let feedback = '', feedbackError = false;
  const inputs = ['problem-category','problem-description','problem-menus','problem-black','problem-second-works','problem-second-crashes'];
  const observations = {'problem-menus':'menus_visible','problem-black':'main_view_black','problem-second-works':'second_window_works','problem-second-crashes':'second_window_crashes'};
  const text = (id, value) => {$(id).textContent = value;};

  function render() {
    $('problem-open').disabled = busy;
    if (!opened) return;
    $('problem-graphics').hidden = $('problem-category').value !== 'graphics';
    for (const id of inputs) $(id).disabled = busy;
    $('problem-prepare').disabled = busy || !isOnline() || !getStatus()?.csrf_token;
    text('problem-prepare', t(busy ? 'Bericht wird erstellt …' : 'Bericht vorbereiten'));
    $('problem-review').hidden = !draft;
    $('problem-download').disabled = !draft || dirty || busy;
    $('problem-copy').disabled = !draft || dirty || busy;
    $('problem-discard').disabled = !draft || busy || !isOnline();
    text('problem-status', t(dirty && draft && !feedbackError ? 'Angaben geändert. Bitte den Bericht erneut vorbereiten.' : feedback));
    $('problem-status').classList.toggle('error', feedbackError);
    if (!draft) return;
    const report = draft.report, email = emailDraft(report, recipient);
    const game = report.diagnostics?.summary?.context?.game_id;
    text('problem-context', `${game === 'msfs2020' ? 'MSFS 2020' : game === 'msfs2024' ? 'MSFS 2024' : t('Keine Installation ausgewählt')} · ${report.created_at}`);
    text('problem-id', report.id);
    text('problem-recipient', recipient || t('Keine Supportadresse eingerichtet. Der Bericht kann lokal gespeichert werden.'));
    text('problem-message', `${t('Betreff')}: ${email.subject}\n\n${email.body}`);
    $('problem-long').hidden = !email.tooLong;
    const allowed = email.href && !dirty && !busy;
    $('problem-mail').hidden = !allowed;
    if (allowed) $('problem-mail').href = email.href;
    else $('problem-mail').removeAttribute('href');
  }

  function accept(value, fill = false) {
    if (!value || (value.draft && (!/^[a-f0-9]{32}$/.test(value.draft.report?.id) ||
        !Object.hasOwn(categories, value.draft.report.category) || typeof value.draft.report.description !== 'string' ||
        !Array.isArray(value.draft.report.observations)))) {
      throw new Error(t('Der lokale Dienst hat keinen gültigen Fehlerbericht geliefert.'));
    }
    draft = value.draft; recipient = value.recipient; dirty = false;
    if (fill && draft) {
      $('problem-category').value = draft.report.category;
      $('problem-description').value = draft.report.description;
      for (const [id, name] of Object.entries(observations)) $(id).checked = draft.report.observations.includes(name);
    }
    feedback = value.unreadable ? 'Der gespeicherte Bericht konnte nicht gelesen werden. Bitte einen neuen Bericht vorbereiten.' :
      draft ? 'Der Bericht ist lokal gespeichert. Es wurde nichts versendet.' : '';
    feedbackError = Boolean(value.unreadable);
  }

  $('problem-open').addEventListener('click', async () => {
    $('problem-form').hidden = false;
    if (opened) { $('problem-description').focus(); return; }
    opened = true; busy = true; render();
    try { accept(await request('/api/problem-reports'), true); }
    catch (error) { feedback = error.message; feedbackError = true; }
    finally { busy = false; render(); $('problem-description').focus(); }
  });
  for (const id of inputs) $(id).addEventListener('input', () => {
    dirty = true; render();
  });
  $('problem-form').addEventListener('submit', async event => {
    event.preventDefault();
    if (busy || !isOnline() || !getStatus()?.csrf_token || !$('problem-form').reportValidity()) return;
    const status = getStatus(), category = $('problem-category').value;
    const body = {category, description:$('problem-description').value,
      runtime_path:status.runtime.path || null,
      observations:category === 'graphics' ? Object.entries(observations).filter(([id]) => $(id).checked).map(([,name]) => name) : []};
    busy = true; feedback = ''; feedbackError = false; render();
    try { accept(await request('/api/problem-reports/prepare', {method:'POST',token:status.csrf_token,body,timeout:30000})); }
    catch (error) { feedback = error.message; feedbackError = true; }
    finally { busy = false; render(); if (draft && !dirty) $('problem-review').scrollIntoView({block:'nearest'}); }
  });
  $('problem-download').addEventListener('click', () => {
    if (!draft || dirty || busy) return;
    const url = URL.createObjectURL(new Blob([reportText(draft.report) + '\n'], {type:'text/plain;charset=utf-8'}));
    const link = document.createElement('a'); link.href = url; link.download = reportFilename(draft.report);
    document.body.append(link); link.click(); link.remove(); setTimeout(() => URL.revokeObjectURL(url), 1000);
  });
  $('problem-copy').addEventListener('click', async () => {
    if (!draft || dirty || busy) return;
    try { await navigator.clipboard.writeText($('problem-message').textContent); feedback = 'Nachricht mit Diagnosedaten kopiert. Füge sie in deine E-Mail ein; ein Anhang ist nicht nötig.'; feedbackError = false; }
    catch { feedback = 'Kopieren nicht verfügbar. Du kannst die Nachricht unten markieren und kopieren.'; feedbackError = true; }
    render();
  });
  $('problem-mail').addEventListener('click', event => {
    if (!draft || dirty || busy || !emailDraft(draft.report, recipient).href) {event.preventDefault(); return;}
    feedback = 'E-Mail-Entwurf angefordert. Prüfe die Nachricht im Mailprogramm und klicke dort auf Senden. Falls kein Entwurf erscheint, nutze E-Mail-Text kopieren.';
    feedbackError = false; render();
  });
  $('problem-discard').addEventListener('click', async () => {
    if (!draft || busy || !getStatus()?.csrf_token) return;
    busy = true; render();
    try {
      accept(await request('/api/problem-reports/discard', {method:'POST',token:getStatus().csrf_token,body:{report_id:draft.report.id}}));
      $('problem-form').reset(); feedback = 'Der lokale Entwurf wurde gelöscht. Heruntergeladene Dateien bleiben erhalten.';
    } catch (error) { feedback = error.message; feedbackError = true; }
    finally { busy = false; render(); }
  });
  return {render};
}
