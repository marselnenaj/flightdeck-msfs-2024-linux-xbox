// SPDX-License-Identifier: MIT
/** @typedef {'setup' | 'fenix' | 'gsx' | 'updates' | 'cloud' | 'launcher' | 'maintenance' | 'proton' | 'store'} Reservation */

// This is UI coordination only; the service remains the authority for actions.
export function createReservations() {
  /** @type {Set<Reservation>} */
  const active = new Set();
  return {
    /** @param {Reservation} name @param {boolean} value */
    set(name, value) {
      if (active.has(name) === value) return false;
      if (value) active.add(name); else active.delete(name);
      return true;
    },
    /** @param {Reservation} name */
    has: name => active.has(name),
    /** @param {...Reservation} excluded */
    anyExcept(...excluded) {
      for (const name of active) if (!excluded.includes(name)) return true;
      return false;
    },
  };
}
