import test from 'node:test';
import {readFileSync} from 'node:fs';
import assert from 'node:assert/strict';
import {catalogs, chooseLanguage, getLanguage, setLanguage, t, plural, locale} from '../i18n.js';
import {formatCount, formatDate, gamePresentation} from '../state.js';

test('query override precedes saved preference and browser language', () => {
  assert.equal(chooseLanguage({query:'en',stored:'de',browser:'de-AT'}),'en');
  assert.equal(chooseLanguage({query:'xx',stored:'de',browser:'en-US'}),'de');
  assert.equal(chooseLanguage({browser:'de-CH'}),'de');
  assert.equal(chooseLanguage({browser:'fr-FR'}),'en');
  assert.equal(chooseLanguage({browser:'en-US'}),'en');
});
test('catalogs have matching keys and named placeholders, no HTML', () => {
  assert.deepEqual(Object.keys(catalogs.de),Object.keys(catalogs.en));
  for (const [key,de] of Object.entries(catalogs.de)) {
    const en=catalogs.en[key]; assert.ok(en.length);
    assert.deepEqual(de.match(/\{\w+\}/g),en.match(/\{\w+\}/g),key);
    assert.equal(/<[^>]+>/.test(en),false,key);
  }
});
test('language changes preserve literal interpolation and reject unsupported values', () => {
  setLanguage('en');assert.equal(getLanguage(),'en');
  assert.equal(t('Gestartet {time}',{time:'<img src=x>'}),'Started <img src=x>');
  assert.equal(setLanguage('fr'),false);assert.equal(getLanguage(),'en');
  setLanguage('de');assert.equal(t('Übersicht'),'Übersicht');
});
test('German and English plural rules handle zero, one and multiple saves', () => {
  for(const [language,expected] of [['de',['0 Dateien','1 Datei','2 Dateien']],['en',['0 files','1 file','2 files']]]) {
    setLanguage(language);
    assert.deepEqual([0,1,2].map(count=>plural(count,'{count} Datei','{count} Dateien',{count})),expected);
  }
  setLanguage('de');
});
test('number, date and game state formatting follows the current locale', () => {
  setLanguage('de');assert.equal(locale(),'de-AT');assert.equal(formatCount(1234),new Intl.NumberFormat('de-AT').format(1234));const de=formatDate('2026-09-17T15:30:00Z');
  setLanguage('en');assert.equal(locale(),'en-GB');assert.equal(formatCount(1234),'1,234');assert.notEqual(formatDate('2026-09-17T15:30:00Z'),de);
  assert.equal(gamePresentation(null).label,'Loading status');setLanguage('de');
});

// Static UI copy must never silently fall back to German in English mode.
test('every static UI translation key has an explicit German and English entry',()=>{
  const html=readFileSync(new URL('../index.html',import.meta.url),'utf8');
  for(const match of html.matchAll(/data-i18n(?:-[a-z-]+)?="([^"]+)"/g)){
    const key=match[1].replaceAll('&amp;','&');
    assert.ok(Object.hasOwn(catalogs.de,key),key);assert.ok(Object.hasOwn(catalogs.en,key),key);
  }
});
