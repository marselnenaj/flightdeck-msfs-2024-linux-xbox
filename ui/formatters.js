// SPDX-License-Identifier: MIT
/** @param {string} locale */
function create(locale) {
  return {
    count: new Intl.NumberFormat(locale),
    decimal: new Intl.NumberFormat(locale, {maximumFractionDigits: 1}),
    date: new Intl.DateTimeFormat(locale, {dateStyle: 'medium', timeStyle: 'short'}),
  };
}
let language = '';
/** @type {ReturnType<typeof create> | undefined} */
let cached;

// Bound the cache to the current locale; switching languages replaces it.
/** @param {string} locale */
export function formatters(locale) {
  if (!cached || locale !== language) {
    cached = create(locale);
    language = locale;
  }
  return cached;
}
