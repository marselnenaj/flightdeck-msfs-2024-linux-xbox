import test from 'node:test';
import assert from 'node:assert/strict';
import {normalizeStatus, actionPermissions, gamePresentation, formatBytes, formatDate, formatCount, normalizeChecks} from '../state.js';

const valid = () => ({
  app: {name: 'Flightdeck', version: 'test'},
  runtime: {configured: true, path: '/synthetic/runtime', ready: true, checks: []},
  game: {state: 'stopped', managed: false, can_start: true, can_stop: false},
  saves: {mode: 'local', available: true, can_backup: true, bytes: 0, files: 0, backups: 0, last_backup: null},
  csrf_token: 'synthetic-csrf',
});

test('only a fresh ready status allows launch', () => {
  const status = normalizeStatus(valid());
  assert.equal(actionPermissions(status, true, null).start, true);
  assert.equal(actionPermissions(status, false, null).start, false);
  assert.equal(actionPermissions(status, true, 'backup').start, false);
  assert.equal(actionPermissions({...status, csrf_token: ''}, true, null).start, false);
});
test('external/unmanaged/unknown sessions cannot be stopped or started', () => {
  for (const mode of ['external', 'unknown', 'invalid']) {
    const raw = valid(); raw.game = {state: mode, managed: true, can_start: true, can_stop: true};
    const permissions = actionPermissions(normalizeStatus(raw), true, null);
    assert.equal(permissions.stop, false); assert.equal(permissions.start, false);
  }
  const raw = valid(); raw.game = {state: 'running', managed: false, can_stop: true};
  assert.equal(actionPermissions(normalizeStatus(raw), true, null).stop, false);
});
test('managed running session has stop but no configuration', () => {
  const raw = valid(); raw.game = {state: 'running', managed: true, can_start: true, can_stop: true};
  const permissions = actionPermissions(normalizeStatus(raw), true, null);
  assert.equal(permissions.stop, true); assert.equal(permissions.configure, false); assert.equal(permissions.start, false);
});
test('unconfigured runtime exposes setup without granting start', () => {
  const raw = valid(); raw.runtime.ready = false; raw.runtime.configured = false;
  const status = normalizeStatus(raw); const permissions = actionPermissions(status, true, null);
  assert.equal(permissions.setup, true); assert.equal(permissions.start, false); assert.equal(permissions.configure, true);
  assert.equal(gamePresentation(status).action, 'Installation einrichten');
});
test('service updates retain only bounded display information', () => {
  const raw = valid();
  raw.service = {update_pending: true, message: 'Finish the active game before updating.', token: 'private'};
  assert.deepEqual(normalizeStatus(raw).service, {update_pending: true, message: raw.service.message});
  raw.service = {update_pending: 'true', message: {unexpected: 'object'}};
  assert.deepEqual(normalizeStatus(raw).service, {update_pending: false, message: ''});
});
test('local backup requires all capability flags', () => {
  for (const [field, value] of [['available', false], ['can_backup', false], ['mode', 'unavailable']]) {
    const raw = valid(); raw.saves[field] = value;
    assert.equal(actionPermissions(normalizeStatus(raw), true, null).backup, false);
  }
});
test('missing data does not fabricate zero or readiness', () => {
  assert.throws(() => normalizeStatus({}));
  const status = normalizeStatus({runtime: {}, game: {}, saves: {}});
  assert.equal(status.game.state, 'unknown'); assert.equal(status.runtime.ready, false);
  assert.equal(formatCount(status.saves.files), '—'); assert.equal(formatBytes(status.saves.bytes), '—');
  assert.equal(formatBytes(0), '0 B'); assert.equal(formatBytes(1024), '1 KiB');
  assert.equal(formatBytes(-1), '—'); assert.equal(formatBytes(Infinity), '—');
  assert.equal(formatDate(null), null); assert.equal(formatDate('invalid'), null);
});
test('check schema is bounded, nonboolean readiness stays unknown', () => {
  const checks = normalizeChecks([{label: '<script>test</script>', detail: 'x'.repeat(1100), ok: 'true'}]);
  assert.equal(checks[0].ok, null); assert.equal(checks[0].detail.length, 1000);
  assert.equal(checks[0].label, '<script>test</script>');
  assert.equal(normalizeChecks(Array(101).fill({ok: true})).length, 100);
});
