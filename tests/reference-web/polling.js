// SPDX-License-Identifier: MIT
// One timer for all status reads. Jobs run in the service even while the UI sleeps.
/** @typedef {{now: () => number, setTimeout: (callback: () => void, delay: number) => number, clearTimeout: (id: number) => void}} Clock */
/** @typedef {{run: () => unknown, interval: () => number | null, delay: number | null, due: number, running: boolean}} Task */

/**
 * @param {{clock?: Clock, visible?: () => boolean, onError?: (error: unknown) => void}} options
 */
export function createPolling({
  clock = {now: () => performance.now(), setTimeout: (fn, ms) => window.setTimeout(fn, ms), clearTimeout: id => window.clearTimeout(id)},
  visible = () => !document.hidden,
  onError = error => console.error(error),
} = {}) {
  /** @type {Map<string, Task>} */
  const tasks = new Map();
  /** @type {number | null} */
  let timer = null;
  let timerDue = Infinity;
  let stopped = false;

  function cancelTimer() {
    if (timer !== null) clock.clearTimeout(timer);
    timer = null;
    timerDue = Infinity;
  }

  // Re-evaluate conditions after a view, job or connection changes. Shorter
  // intervals take effect promptly; unchanged reads retain their deadline.
  function update(wake = false) {
    if (stopped || !visible()) { cancelTimer(); return; }
    const now = clock.now();
    let next = Infinity;
    for (const task of tasks.values()) {
      const delay = task.interval();
      if (delay !== null && (!Number.isFinite(delay) || delay <= 0)) throw new RangeError('Invalid polling interval');
      if (delay !== task.delay) {
        task.due = delay === null ? Infinity : task.delay === null ? now + delay : Math.min(task.due, now + delay);
        task.delay = delay;
      }
      if (delay === null || task.running) continue;
      if (wake) task.due = now;
      next = Math.min(next, task.due);
    }
    if (timer !== null && next === timerDue) return;
    cancelTimer();
    if (Number.isFinite(next)) {
      timerDue = next;
      timer = clock.setTimeout(tick, Math.max(0, next - now));
    }
  }

  function tick() {
    timer = null;
    timerDue = Infinity;
    if (stopped || !visible()) return;
    const now = clock.now();
    for (const task of tasks.values()) {
      // Recheck at dispatch: a pending mutation may have disabled this read.
      if (task.running || task.due > now || task.interval() === null) continue;
      task.running = true;
      Promise.resolve().then(task.run).catch(onError).finally(() => {
        task.running = false;
        task.delay = task.interval();
        task.due = task.delay === null ? Infinity : clock.now() + task.delay;
        update();
      });
    }
    update();
  }

  return {
    /** @param {string} name @param {() => unknown} run @param {() => number | null} interval */
    add(name, run, interval) {
      if (tasks.has(name)) throw new Error(`Duplicate poll: ${name}`);
      tasks.set(name, {run, interval, delay: null, due: Infinity, running: false});
      update();
    },
    update,
    wake: () => update(true),
    stop() { stopped = true; cancelTimer(); tasks.clear(); },
  };
}
