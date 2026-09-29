import test from 'node:test';
import assert from 'node:assert/strict';
import {setLanguage} from '../i18n.js';
import {emailDraft, reportFilename, reportText} from '../problem-reports.js';

const report = {schema:1,id:'1234567890abcdef1234567890abcdef',created_at:'2026-09-29T12:00:00Z',
  category:'graphics',description:'Menüs & Welt: schwarz? 100% ✈\nBcc: this stays in the body',observations:['main_view_black'],
  system:{distribution:'linuxmint',release:'22.3'},diagnostics:{summary:{context:{game_id:'msfs2024',launcher_version:'0.1.9'},
  graphics:{devices:[{name:'NVIDIA GeForce RTX 5060 Ti',driver_version:'595.91.07'}]},cloud_sync:{error_code:'transport',error_details:{http_status:503}}}}};

test('mail draft includes the full diagnostic text without requiring an attachment',()=>{
  setLanguage('en');const email=emailDraft(report,'contact@flightdeck-app.com');const url=new URL(email.href);
  assert.equal(url.pathname,'contact@flightdeck-app.com');
  assert.deepEqual([...url.searchParams.keys()],['subject','body']);
  assert.equal(url.searchParams.get('body').replaceAll('\r\n','\n'),email.body);
  for(const value of [report.description,report.id,'595.91.07','RTX 5060 Ti','503','linuxmint','0.1.9'])assert.ok(email.body.includes(value),value);
  assert.equal(email.body,reportText(report));assert.ok(email.body.startsWith('Flightdeck problem report'));
  assert.equal(reportFilename(report),`flightdeck-report-${report.id}.txt`);
});

test('long multibyte description never silently disappears from email fallback',()=>{
  const long={...report,description:'✈'.repeat(4000)};const email=emailDraft(long,'contact@flightdeck-app.com');
  assert.equal(email.href,null);assert.equal(email.tooLong,true);assert.ok(email.body.includes(long.description));assert.ok(email.body.includes('595.91.07'));
});

test('recipient cannot add headers, extra recipients or another URL scheme',()=>{
  for(const recipient of [null,'','https://example.com','x@y.com?bcc=a@b.com','x@y.com\r\nBcc:a@b.com','x@y.com,a@b.com','x@y..com'])assert.equal(emailDraft(report,recipient).href,null,recipient);
});

test('localization changes presentation without mutating the frozen report',()=>{
  const before=JSON.stringify(report);setLanguage('de');assert.ok(reportText(report).includes('Automatisch erfasste Diagnosedaten'));
  setLanguage('en');assert.ok(reportText(report).includes('Automatically collected diagnostics'));assert.equal(JSON.stringify(report),before);
});
