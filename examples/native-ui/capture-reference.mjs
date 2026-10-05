// Development-only reference capture. The Rust application never uses this file.
import assert from 'node:assert/strict';
import {readFile, writeFile, mkdir, mkdtemp, rm} from 'node:fs/promises';
import {spawn} from 'node:child_process';
import {tmpdir} from 'node:os';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const artifacts = resolve(process.env.FLIGHTDECK_UI_ARTIFACTS ?? join(root, 'build/native-ui-preview'));
const source = await readFile(join(root, 'tests/reference-web/tests/browser-test.mjs'), 'utf8');
const version = (await readFile(join(root, 'Cargo.toml'), 'utf8')).match(/^version = "([^"]+)"/m)?.[1];
assert.ok(version, 'Launcher package version missing');
const start = source.indexOf('  // Page.navigate acknowledges');
const cleanup = source.lastIndexOf('} catch(error) {');
assert.ok(start > 0 && cleanup > start, 'Browser fixture structure changed; review reference capture');
assert.ok(source.includes('let startupFixture = true;'), 'Startup fixture changed');
let fixture = source.slice(0, start)
  .replace('let startupFixture = true;', 'let startupFixture = false;')
  .replace("app:{name:'Flightdeck',version:'0.1.0'}", "app:{name:'Flightdeck',version:" + JSON.stringify(version) + '}');
fixture += `
  await call('Runtime.enable'); await call('Page.enable'); await call('Network.enable');
  await call('Emulation.setDeviceMetricsOverride', {width:1536,height:1024,deviceScaleFactor:1,mobile:false});
  await call('Page.navigate', {url:origin+'/?lang=de'});
  await until(()=>evaluate("document.getElementById('game-state')?.textContent === 'Bereit zum Start'"), 'Ready status missing');
  await evaluate('document.fonts.ready');
  for (const year of ['2024','2020']) {
    if (year==='2020') await click('version-msfs'+year);
    await until(()=>evaluate("document.getElementById('version-msfs"+year+"').getAttribute('aria-pressed')==='true'"), 'Edition selection missing');
    await sleep(800);
    for (const [width,height] of [[960,700],[1280,900],[1536,1024],[1920,1080]]) {
      await call('Emulation.setDeviceMetricsOverride', {width,height,deviceScaleFactor:1,mobile:false});
      await evaluate('window.scrollTo(0,0)');
      await screenshot('web-'+year+'-'+width+'.png');
      const layout=await evaluate("[...document.querySelectorAll('.page-header,.version-switch,.launch-panel,.launch-title,.overview-details,.readiness,.save-summary')].map(e=>({selector:e.className,rect:e.getBoundingClientRect().toJSON(),font:getComputedStyle(e).font,text:e.innerText}))");
      await writeFile(join(artifacts,'web-layout-'+year+'-'+width+'.json'), JSON.stringify(layout,null,2)+'\\n');
    }
  }
  assert.deepEqual(errors,[]); assert.deepEqual(consoleIssues,[]); assert.deepEqual(externalRequests,[]);
  console.log('Reference capture passed: synthetic data, no page errors or external page requests');
  })();
` + source.slice(cleanup);

const temp = await mkdtemp(join(tmpdir(), 'flightdeck-native-reference-'));
await mkdir(artifacts, {recursive:true});
try {
  const file = join(temp, 'reference.mjs');
  await writeFile(file, fixture);
  const child = spawn(process.execPath, [file], {stdio:'inherit', env:{...process.env,FLIGHTDECK_UI_SOURCE:join(root,'tests/reference-web'),FLIGHTDECK_UI_ARTIFACTS:artifacts}});
  const code = await new Promise((resolve,reject)=>{child.once('error',reject);child.once('exit',resolve);});
  assert.equal(code, 0, 'Reference capture failed');
} finally {
  await rm(temp, {recursive:true,force:true});
}

const comparisons = ['2024','2020'].flatMap(year => [1536,1280,960,1920].map(width =>
  '<section><h2>MSFS '+year+' · '+width+' px</h2><div class="pair"><figure><figcaption>Original · Browser</figcaption><a href="web-'+year+'-'+width+'.png"><img src="web-'+year+'-'+width+'.png" alt="Original '+year+'"></a></figure><figure><figcaption>Rust · native widgets</figcaption><a href="rust-'+year+'-'+width+'.png"><img src="rust-'+year+'-'+width+'.png" alt="Rust '+year+'"></a></figure></div></section>'
)).join('\n');
await writeFile(join(artifacts,'comparison.html'), '<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Flightdeck · Native Rust comparison</title><style>body{font:16px system-ui;background:#09131d;color:#f3f6fb;margin:24px}h1{font-size:26px}h2{font-size:19px;margin-top:36px}p,figcaption{color:#a6b8c9}.pair{display:grid;grid-template-columns:1fr 1fr;gap:16px}figure{margin:0}figcaption{margin:8px 0}img{width:100%;height:auto;border:1px solid #213b4b}a{color:#64e7f2}@media(max-width:700px){.pair{grid-template-columns:1fr}}</style><h1>Flightdeck: browser and native Rust overview</h1><p>Same synthetic data, original local assets and logical viewport sizes. Click any screenshot for full resolution. This is a UI prototype, not a migrated launcher.</p>'+comparisons+'</html>');
console.log('Comparison: '+join(artifacts,'comparison.html'));
