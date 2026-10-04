import test from 'node:test';
import assert from 'node:assert/strict';
import {createPolling} from '../polling.js';
import {createReservations} from '../reservations.js';
import {formatters} from '../formatters.js';

function fixture() {
  let now = 0, serial = 0, visible = true;
  const timers = new Map(), errors = [];
  const flush = async () => { for (let n = 0; n < 8; n++) await Promise.resolve(); };
  const polling = createPolling({
    clock: {now: () => now, setTimeout: (callback, delay) => {
      const id = ++serial; timers.set(id, {callback, due: now + delay}); return id;
    }, clearTimeout: id => timers.delete(id)},
    visible: () => visible, onError: error => errors.push(error),
  });
  return {polling, timers, errors, flush, get scheduled() { return serial; },
    show(value) { visible = value; polling.wake(); },
    async advance(ms) {
      const end = now + ms;
      let remaining = 1000;
      while (timers.size) {
        assert.ok(--remaining > 0, 'Scheduler must not spin');
        const [id, timer] = [...timers].sort((a,b) => a[1].due - b[1].due)[0];
        if (timer.due > end) break;
        now = timer.due; timers.delete(id); timer.callback(); await flush();
      }
      now = end; await flush();
    },
  };
}

test('idle panels share one timer and six reads per minute per endpoint', async () => {
  const f = fixture(); let reads = 0;
  for (const name of ['status','setup','maintenance','proton']) f.polling.add(name, () => reads++, () => 10000);
  assert.equal(f.timers.size, 1);
  await f.advance(60000);
  assert.equal(reads, 24);
  assert.equal(f.timers.size, 1);
});

test('active jobs refresh promptly and return to the idle cadence', async () => {
  const f = fixture(); let active = false, reads = 0;
  f.polling.add('job', () => reads++, () => active ? 1500 : 10000);
  await f.advance(1000); active = true; f.polling.update();
  await f.advance(1499); assert.equal(reads, 0);
  await f.advance(1); assert.equal(reads, 1);
  await f.advance(3000); assert.equal(reads, 3);
  active = false;
  await f.advance(1500); assert.equal(reads, 4);
  await f.advance(9999); assert.equal(reads, 4);
  await f.advance(1); assert.equal(reads, 5);
});

test('hidden windows stop every poll including active jobs and refresh once on return', async () => {
  const f = fixture(); let reads = 0;
  f.polling.add('job', () => reads++, () => 1500);
  f.show(false); assert.equal(f.timers.size, 0);
  await f.advance(60000); assert.equal(reads, 0);
  f.show(true); await f.advance(0); assert.equal(reads, 1);
  await f.advance(1499); assert.equal(reads, 1);
});

test('slow reads never overlap or block independent endpoints', async () => {
  const f = fixture(); let release, slow = 0, fast = 0;
  f.polling.add('slow', () => {slow++; return new Promise(resolve => release = resolve);}, () => 1500);
  f.polling.add('fast', () => fast++, () => 1500);
  await f.advance(6000);
  assert.equal(slow, 1); assert.equal(fast, 4);
  f.polling.wake(); await f.advance(0); assert.equal(slow, 1);
  release(); await f.flush();
  await f.advance(1499); assert.equal(slow, 1);
  await f.advance(1); assert.equal(slow, 2);
  release(); await f.flush(); f.polling.stop();
});

test('leaving a view disables its scheduled read; explicit re-entry enables it', async () => {
  const f = fixture(); let enabled = true, reads = 0;
  f.polling.add('view', () => reads++, () => enabled ? 10000 : null);
  enabled = false; f.polling.update();
  await f.advance(60000); assert.equal(reads, 0); assert.equal(f.timers.size, 0);
  enabled = true; f.polling.wake(); await f.advance(0); assert.equal(reads, 1);
});

test('a mutation beginning just before dispatch prevents the stale read', async () => {
  const f = fixture(); let pending = false, reads = 0;
  f.polling.add('view', () => reads++, () => pending ? null : 10000);
  pending = true; await f.advance(10000); assert.equal(reads, 0);
  pending = false; f.polling.update(); await f.advance(10000); assert.equal(reads, 1);
});

test('a failed read reports its error and remains retryable', async () => {
  const f = fixture(); let reads = 0;
  f.polling.add('retry', () => {if (++reads === 1) throw new Error('offline');}, () => 1500);
  await f.advance(3000); assert.equal(reads, 2); assert.equal(f.errors[0].message, 'offline');
});

test('completion after hiding or disposal cannot restart timers', async () => {
  const f = fixture(); let release;
  f.polling.add('job', () => new Promise(resolve => release = resolve), () => 1500);
  await f.advance(1500); f.show(false); release(); await f.flush();
  assert.equal(f.timers.size, 0);
  f.show(true); await f.advance(0); f.polling.stop(); release(); await f.flush();
  f.polling.wake(); assert.equal(f.timers.size, 0);
});

test('repeated reconciliation retains the deadline instead of starving reads', async () => {
  const f = fixture(); let reads = 0;
  f.polling.add('status', () => reads++, () => 10000);
  for (let n = 0; n < 20; n++) f.polling.update();
  assert.equal(f.scheduled, 1, 'Unchanged renders must not replace the timer');
  for (let n = 0; n < 10; n++) {await f.advance(1000); f.polling.update();}
  assert.equal(reads, 1);
});

test('duplicate endpoint registration is rejected', () => {
  const f = fixture(); f.polling.add('status', () => {}, () => 10000);
  assert.throws(() => f.polling.add('status', () => {}, () => 10000), /Duplicate poll/);
});

test('each job releases only its own reservation and unchanged values are quiet', () => {
  const reservations = createReservations();
  assert.equal(reservations.set('setup', false), false);
  assert.equal(reservations.set('setup', true), true);
  assert.equal(reservations.set('setup', true), false);
  assert.equal(reservations.anyExcept('setup'), false);
  reservations.set('cloud', true);
  assert.equal(reservations.anyExcept('setup'), true);
  reservations.set('setup', false);
  assert.equal(reservations.anyExcept(), true);
  assert.equal(reservations.has('cloud'), true);
  reservations.set('cloud', false);
  assert.equal(reservations.anyExcept(), false);
});

test('a new reservation blocks existing actions without extending an exclusion list', () => {
  const reservations = createReservations();
  for (const name of ['setup','fenix','gsx','updates','cloud','launcher','maintenance','proton','store']) {
    reservations.set(name, true);
    assert.equal(reservations.anyExcept(), true);
    assert.equal(reservations.anyExcept(name), false);
    reservations.set(name, false);
  }
});

test('cached formatters preserve output and switch locales immediately', () => {
  const german = formatters('de-AT');
  assert.equal(formatters('de-AT'), german);
  assert.equal(german.decimal.format(1234.56), new Intl.NumberFormat('de-AT', {maximumFractionDigits:1}).format(1234.56));
  const english = formatters('en-US');
  assert.notEqual(english, german);
  assert.equal(english.count.format(12345), '12,345');
  assert.equal(formatters('de-AT').decimal.format(1234.56), german.decimal.format(1234.56));
});
