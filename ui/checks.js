// SPDX-License-Identifier: MIT
/** @typedef {{label: string, detail: string, ok: boolean | null}} Check */
/** @typedef {{empty: string, ready: string, failed: string, unknown: string}} Labels */

export function createCheckRenderer() {
  /** @type {WeakMap<HTMLElement, string>} */
  const previous = new WeakMap();
  /** @param {HTMLElement} target @param {readonly Check[]} checks @param {Labels} labels */
  return (target, checks, labels) => {
    const key = JSON.stringify([checks, labels]);
    if (previous.get(target) === key) return;
    const document = target.ownerDocument;
    const rows = checks.map(check => {
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
      result.textContent = check.ok === true ? labels.ready : check.ok === false ? labels.failed : labels.unknown;
      row.append(marker, content, result);
      return row;
    });
    if (!rows.length) {
      const row = document.createElement('li'); row.className = 'empty-state'; row.textContent = labels.empty; rows.push(row);
    }
    target.replaceChildren(...rows);
    previous.set(target, key);
  };
}
