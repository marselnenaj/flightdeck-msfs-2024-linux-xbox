// Isolated synthetic API + actual Chromium. No real launcher/runtime/account access.
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {spawn} from 'node:child_process';
import {mkdtemp, readFile, writeFile, mkdir, rm, readdir} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {resolve, dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';

const base = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const artifacts = process.env.FLIGHTDECK_UI_ARTIFACTS ? resolve(process.env.FLIGHTDECK_UI_ARTIFACTS) : await mkdtemp(join(tmpdir(), 'flightdeck-ui-artifacts-'));
await mkdir(artifacts, {recursive: true});
const temp = await mkdtemp(join(tmpdir(), 'flightdeck-ui-'));
const csrf = 'fixture-CSRF-MUST-NOT-APPEAR-IN-DOM-OR-EXPORT';
const checks = [
  {id:'game',label:'Spielinstallation',ok:true,detail:'MSFS 2024 und MicrosoftGame.Config vorhanden.'},
  {id:'runner',label:'Kompatibilitäts-Runtime',ok:true,detail:'Wine-Runner und Xbox-Dienst sind verfügbar.'},
  {id:'saves',label:'Lokaler Spielstandspeicher',ok:true,detail:'Lokaler Speicher für Cloud-Abgleich und Sicherungen aktiviert.'},
];
const englishChecks = [
  {id:'game',label:'Game installation',ok:true,detail:'MSFS 2024 and MicrosoftGame.Config are present.'},
  {id:'runner',label:'Compatibility runtime',ok:true,detail:'Wine runner and Xbox service are available.'},
  {id:'saves',label:'Local save storage',ok:true,detail:'Local storage enabled for cloud sync and backups.'},
];
const localizeChecks=(items,language)=>language==='en'?items.map(item=>{
  const translated=englishChecks.find(other=>other.id===item.id);
  return translated&&item.id==='game'&&item.detail.includes('2020')
    ? {...translated,detail:'MSFS 2020 and MicrosoftGame.Config are present.'}
    : translated??item;
}):items;
const autoIdle=()=>({enabled:true,state:'idle',phase:null,message:'',error_code:null,can_retry:false,can_play_local:false,can_cancel:false,request_id:null,last_synced_at:null,conflict:false,summary:null});
const autoRequest='123456781234423482341234567890ab';
let automaticFixture=false;
let status = {
  app:{name:'Flightdeck',version:'0.1.0'},cloud:autoIdle(),
  runtime:{configured:true,path:'/opt/flightdeck-fixture/MSFS2024',game_id:'msfs2024',game_name:'Microsoft Flight Simulator 2024',ready:true,checks},
  versions:{msfs2024:{path:'/opt/flightdeck-fixture/MSFS2024',installed:true,ready:true},
    msfs2020:{path:'/opt/flightdeck-fixture/MSFS2020',installed:true,ready:true}},
  game:{state:'stopped',managed:false,can_start:true,can_stop:false,started_at:null,exit_code:null},
  saves:{mode:'local',available:true,bytes:24576,files:3,backups:1,can_backup:true,last_backup:{name:'save-backup-test.zip',created_at:'2026-09-17T16:00:00Z'}},
  support:{level:'experimental',cloud_saves:false},csrf_token:csrf,
};
let apiUnavailable = false, failNext = false, nextCheckState = 'ready', switchDelay = 0;
let statusBarrier = null, releaseStatus = null, statusWaiting = false;
const apiRequests = [];
const posts = [], externalRequests = [], errors = [], results = [];
const startupRequests = [], consoleIssues = [];
let startupFixture = true;
let discovered=[{name:'Microsoft Flight Simulator 2024',path:'/synthetic/path with spaces',ready:true,configured:false,checks}];
let setup = {available:true,install_available:false,prepare_available:true,state:'idle',job:null,defaults:{mode:'existing',runtime_path:status.runtime.path,market:'AT',local_saves:true,destination_path:'/synthetic/new-msfs'}};
let mods={state:'ready',message:'',folder_path:'/synthetic/Community',can_open:true,mods:[],count:0,scanned_count:0,limited:false};
let modsUnavailable=false;
let fenix={state:'available',installed:false,configured:false,settings_ready:false,idle:true,fenix_installed:false,manager_installed:false,can_restore:false,can_change:true,fenix_running:false,can_stop:false,job:null};
let gameUpdate={integrity:{available:true,can_check:true,result:null},can_repair:true,available:true,installed_version:'1.8.16.0',latest_version:null,update_available:null,can_check:true,can_start:false,can_rollback:false,auth_required:false};
let updateUnavailable=false,updateDelay=0,updateReplies=0,updateWaiting=false,updateBarrier=null,releaseUpdate=null;
const launcherDefault=()=>({managed:true,installed_version:'0.1.0',latest_version:null,update_available:null,check_id:null,checked_at:null,notes:'',unavailable_reason:'',pending_restart:false,can_check:true,can_install:false,can_restart:false,can_rollback:false,busy:false,job:null});
let launcherUpdate=launcherDefault(),launcherRestartFails=false;
let launcherUnavailable=false,launcherBarrier=null,releaseLauncher=null,launcherWaiting=false;

let cloudData={available:true,mode:'download_and_import',sync_supported:false,can_check:true,can_download:true,can_prepare_import:true,can_import:false,can_cancel:false,plan:null,job:null};
let cloudReplies=0,cloudUnavailable=false,cloudBarrier=null,releaseCloud=null,cloudWaiting=false;
const files = new Set(['launcher-updates.js','notices.js','fenix.js','cloud-saves.js','manrope-variable.woff2','updates.js','mods.js','index.html','styles.css','app.js','setup.js','state.js','i18n.js','mark.svg','flight-panorama.png','flight-panorama-2020.png']);
const server = createServer(async (req,res) => {
  const url = new URL(req.url, 'http://localhost');
  if (url.pathname.startsWith('/api/')) {
    const language=req.headers['accept-language'];apiRequests.push({method:req.method,path:url.pathname,language});
    res.setHeader('Content-Type','application/json'); res.setHeader('Cache-Control','no-store');
    if (apiUnavailable) {res.writeHead(503); res.end(JSON.stringify({ok:false,error:'Fixture offline'}));return;}
    if (req.method === 'GET') {
      if (url.pathname === '/api/status') {if(statusBarrier){statusWaiting=true;await statusBarrier;}res.end(JSON.stringify({...status,runtime:{...status.runtime,checks:localizeChecks(status.runtime.checks,language)}}));return;}
      if (url.pathname === '/api/setup/discover') {res.end(JSON.stringify({ok:true,runtimes:discovered,checked_count:discovered.length,limited:false}));return;}
      if (url.pathname === '/api/setup') {res.end(JSON.stringify({...setup,job:setup.job?{...setup.job,checks:localizeChecks(setup.job.checks,language),message:language==='en'?'Synthetic files checked.':setup.job.message}:null}));return;}
      if (url.pathname === '/api/launcher-update') {
        if(launcherBarrier){launcherWaiting=true;await launcherBarrier;}
        if(launcherUnavailable){res.writeHead(503);res.end(JSON.stringify({ok:false,error:'Synthetic launcher status unavailable'}));return;}
        const busy=status.game.state!=='stopped';
        res.end(JSON.stringify({...launcherUpdate,busy,can_install:launcherUpdate.can_install&&!busy,can_restart:launcherUpdate.can_restart&&!busy,can_rollback:launcherUpdate.can_rollback&&!busy}));return;
      }
      if (url.pathname === '/api/game-update') {if(updateDelay)await sleep(updateDelay);if(updateBarrier){updateWaiting=true;await updateBarrier;}updateReplies++;if(updateUnavailable){res.writeHead(503);res.end(JSON.stringify({ok:false,error:'Synthetic update status unavailable'}));return;}res.end(JSON.stringify({...gameUpdate,job:setup.job?.mode==='update'?setup.job:null}));return;}
      if (url.pathname === '/api/cloud-saves') {if(cloudBarrier){cloudWaiting=true;await cloudBarrier;}cloudReplies++;if(cloudUnavailable){res.writeHead(404);res.end(JSON.stringify({ok:false,error:'Synthetic cloud component unavailable'}));return;}res.end(JSON.stringify({...cloudData,automatic:status.cloud}));return;}
      if (url.pathname === '/api/fenix') {res.end(JSON.stringify({...fenix,runtime_path:status.runtime.path,busy:status.game.state!=='stopped',can_change:fenix.can_change&&status.game.state==='stopped'}));return;}
      if (url.pathname === '/api/mods') {if(modsUnavailable){res.writeHead(503);res.end(JSON.stringify({ok:false,error:'Synthetic inventory unavailable'}));return;}res.end(JSON.stringify(mods));return;}
      if (url.pathname === '/api/diagnostics') {res.end(JSON.stringify({summary:{Runtime:'Bereit',Speichermodus:'Lokal',Experimentell:true,store_calls:[{method:'XStoreShowPurchaseUIAsync',hresult:'80004001'}],cloud_sync:{state:'attention',phase:'after_exit',error_code:'transport',error_details:{http_status:503}},graphics:{status:'ready',session:'wayland',devices:[{name:'NVIDIA GeForce RTX 4060',vendor_id:4318,type:2,api_version:'1.3.280',driver_version:'580.126.9.0'}]}},checks,generated_at:'2026-09-17T17:00:00Z',csrf_token:csrf,private_log:'MUST-NOT-EXPORT'}));return;}
    } else if (req.method === 'POST') {
      let body = ''; for await (const chunk of req) body += chunk;
      if(url.pathname==='/api/updates/check-startup') {
        assert.equal(req.headers['x-flightdeck-token'],csrf);assert.deepEqual(JSON.parse(body),{});
        startupRequests.push({path:url.pathname});
        if(startupFixture) {
          launcherUpdate={...launcherUpdate,latest_version:'0.1.5',update_available:true,check_id:'startup-fixture',checked_at:'2026-09-25T12:00:00Z',can_install:true};
          gameUpdate={...gameUpdate,latest_version:'1.9.0.0',update_available:true,can_start:false};
        }
        res.end(JSON.stringify({ok:true}));return;
      }
      posts.push({path:url.pathname,token:req.headers['x-flightdeck-token'],body:JSON.parse(body)});
      if (req.headers['x-flightdeck-token'] !== csrf) {res.writeHead(403);res.end(JSON.stringify({ok:false,error:'Missing fixture CSRF'}));return;}
      if (failNext) {failNext = false;res.writeHead(400);res.end(JSON.stringify({ok:false,error:'<img src=x onerror="window.injected=1"> Backend-Fehler'}));return;}
      if(url.pathname==='/api/launcher-update/check') {
        assert.deepEqual(JSON.parse(body),{});
        launcherUpdate={...launcherUpdate,latest_version:launcherUpdate.installed_version==='0.1.5'?'0.1.6':'0.1.5',update_available:true,check_id:'launcher-check-fixture',checked_at:'2026-09-24T20:00:00Z',can_install:launcherUpdate.managed,notes:'New update\n<img src=x onerror="window.launcherInjected=1">',job:null};
        res.end(JSON.stringify({ok:true}));return;
      }
      if(url.pathname==='/api/launcher-update/install') {
        assert.deepEqual(JSON.parse(body),{check_id:'launcher-check-fixture'});
        launcherUpdate={...launcherUpdate,can_check:false,can_install:false,job:{id:'launcher-install-fixture',operation:'install',state:'running',phase:'downloading',can_cancel:true,progress:42,received:42000,total:100000,message:'Synthetic verified download'}};
        res.end(JSON.stringify({ok:true}));return;
      }
      if(url.pathname==='/api/launcher-update/cancel') {
        assert.deepEqual(JSON.parse(body),{job_id:'launcher-install-fixture'});
        launcherUpdate={...launcherUpdate,can_check:true,can_install:true,job:{...launcherUpdate.job,state:'cancelled',can_cancel:false}};
        res.end(JSON.stringify({ok:true}));return;
      }
      if(url.pathname==='/api/launcher-update/rollback') {
        assert.deepEqual(JSON.parse(body),{});
        launcherUpdate={...launcherUpdate,pending_restart:true,can_check:false,can_install:false,can_rollback:false,can_restart:true,job:{id:'rollback-fixture',operation:'rollback',state:'complete',message:'Synthetic previous version ready'}};
        res.end(JSON.stringify({ok:true}));return;
      }
      if(url.pathname==='/api/launcher-update/restart') {
        assert.deepEqual(JSON.parse(body),{});
        if(launcherRestartFails){
          launcherRestartFails=false;launcherUpdate={...launcherUpdate,job:{id:'restart-failed',operation:'restart',state:'failed',error:'Synthetic launcher restart failed'}};
          res.end(JSON.stringify({ok:true}));return;
        }
        status.app.version='0.1.5';launcherUpdate={...launcherDefault(),installed_version:'0.1.5',latest_version:'0.1.5',update_available:false,can_rollback:true};
        res.end(JSON.stringify({ok:true}));return;
      }
      if (url.pathname === '/api/fenix/install') {assert.deepEqual(JSON.parse(body),{bundle_path:''});fenix={...fenix,can_change:false,job:{state:'running',message:'Synthetic Fenix setup'}};res.end(JSON.stringify({ok:true,job_id:'fenix-fixture'}));return;}
      if (url.pathname === '/api/fenix/configure') {assert.deepEqual(JSON.parse(body),{});fenix={...fenix,configured:true,job:{state:'complete',operation:'configure',message:'Synthetic displays configured'}};res.end(JSON.stringify({ok:true,job_id:'fenix-configure-fixture'}));return;}
      if (url.pathname === '/api/fenix/stop') {assert.deepEqual(JSON.parse(body),{});fenix={...fenix,fenix_running:false,can_stop:false,idle:true,can_change:true,job:{state:'complete',operation:'open',message:'Fenix wurde beendet.'}};status.game={...status.game,state:'stopped'};res.end(JSON.stringify({ok:true,job_id:'fenix-open-fixture'}));return;}
      if (url.pathname === '/api/game/select') {
        if (switchDelay) await sleep(switchDelay);
        const gameId=JSON.parse(body).game_id;
        assert.ok(status.versions[gameId]?.ready);
        status.runtime={...status.runtime,path:status.versions[gameId].path,game_id:gameId,
          game_name:gameId==='msfs2020'?'Microsoft Flight Simulator 2020':'Microsoft Flight Simulator 2024',
          checks:checks.map(item=>item.id==='game'?{...item,detail:`MSFS ${gameId==='msfs2020'?'2020':'2024'} und MicrosoftGame.Config vorhanden.`}:item)};
        res.end(JSON.stringify({ok:true}));return;
      }
      if (['/api/cloud-saves/check','/api/cloud-saves/download','/api/cloud-saves/prepare-import'].includes(url.pathname)) {assert.deepEqual(JSON.parse(body),{});const operation=url.pathname.split('/').at(-1);cloudData={...cloudData,can_check:false,can_download:false,can_cancel:true,job:{id:'cloud-'+operation,operation,state:'running',message:'Synthetic cloud request running.',result:null}};res.end(JSON.stringify({ok:true,job_id:cloudData.job.id}));return;}
      if (url.pathname === '/api/cloud-saves/import') {assert.deepEqual(JSON.parse(body),{plan_id:cloudData.plan.id,choice:'cloud'});cloudData={...cloudData,plan:null,can_import:false,can_cancel:true,job:{id:'cloud-import',operation:'import',state:'running',message:'',result:null}};res.end(JSON.stringify({ok:true,job_id:'cloud-import'}));return;}
      if (url.pathname === '/api/cloud-saves/upload') {assert.deepEqual(JSON.parse(body),{plan_id:cloudData.plan.id,choice:'local'});cloudData={...cloudData,plan:null,can_upload:false,can_cancel:true,job:{id:'cloud-upload',operation:'upload',state:'running',message:'',result:null}};res.end(JSON.stringify({ok:true,job_id:'cloud-upload'}));return;}
      if (url.pathname === '/api/cloud-saves/restore') {assert.deepEqual(JSON.parse(body),{backup_id:cloudData.restore_id});cloudData={...cloudData,restore_id:null,can_restore:false,plan:null,can_cancel:true,job:{id:'cloud-restore',operation:'restore',state:'running',message:'',result:null}};res.end(JSON.stringify({ok:true,job_id:'cloud-restore'}));return;}
      if (url.pathname === '/api/cloud-saves/discard-plan') {assert.deepEqual(JSON.parse(body),{plan_id:cloudData.plan.id});cloudData={...cloudData,plan:null,can_import:false};res.end(JSON.stringify({ok:true}));return;}
      if (url.pathname === '/api/cloud-saves/cancel') {assert.equal(JSON.parse(body).job_id,cloudData.job.id);cloudData={...cloudData,can_check:true,can_download:true,can_cancel:false,job:{...cloudData.job,state:'cancelled',result:null}};res.end(JSON.stringify({ok:true}));return;}
      if(['/api/cloud-saves/retry','/api/cloud-saves/play-local','/api/cloud-saves/cancel-auto','/api/cloud-saves/resolve'].includes(url.pathname)) {
        const data=JSON.parse(body);assert.equal(data.request_id,status.cloud.request_id);
        if(url.pathname.endsWith('/resolve'))assert.ok(['cloud','local'].includes(data.choice));else assert.deepEqual(data,{request_id:status.cloud.request_id});
        if(url.pathname.endsWith('/cancel-auto')){status.cloud=autoIdle();status.game.can_start=true;}
        else if(url.pathname.endsWith('/play-local')){status.cloud={...autoIdle(),state:'playing',message:'Synthetic local session.',request_id:autoRequest};status.game={state:'running',managed:true,can_start:false,can_stop:true};}
        else {status.cloud={...autoIdle(),state:'syncing',phase:'before_start',request_id:autoRequest,can_cancel:true};status.game.can_start=false;}
        res.end(JSON.stringify({ok:true}));return;
      }
      if(automaticFixture && url.pathname==='/api/launch') {status.cloud={...autoIdle(),state:'syncing',phase:'before_start',request_id:autoRequest,can_cancel:true};status.game={state:'stopped',managed:false,can_start:false,can_stop:false};res.end(JSON.stringify({ok:true,cloud_sync:true,request_id:autoRequest}));return;}
      if(automaticFixture && url.pathname==='/api/stop') {status.cloud={...autoIdle(),state:'syncing',phase:'after_exit',request_id:autoRequest};status.game={state:'stopped',managed:false,can_start:false,can_stop:false};res.end(JSON.stringify({ok:true}));return;}
      if (url.pathname === '/api/launch') status.game = {state:'running',managed:true,can_start:false,can_stop:true,started_at:'2026-09-17T17:00:00Z',exit_code:null};
      if (url.pathname === '/api/stop') status.game = {state:'stopped',managed:false,can_start:true,can_stop:false,started_at:null,exit_code:0};
      if (url.pathname === '/api/config') status.runtime.path = JSON.parse(body).runtime_path;
      if (url.pathname === '/api/saves/backup') {status.saves.backups++;status.saves.last_backup={name:'new-synthetic-backup.zip',created_at:'2026-09-17T18:00:00Z'};}
      if (url.pathname === '/api/mods/open-folder') {assert.deepEqual(JSON.parse(body),{});res.end(JSON.stringify({ok:true,message:'Synthetic file manager requested'}));return;}
      if (url.pathname === '/api/game-update/verify') {assert.deepEqual(JSON.parse(body),{});setup.job={id:'verify-fixture',mode:'update',operation:'verify',state:'checking',phase:'integrity_check',message:'Synthetic local file check.',checks:[],progress:null};status.game.can_start=false;res.end(JSON.stringify({ok:true,job:setup.job}));return;}
      if (url.pathname === '/api/game-update/repair/check') {gameUpdate={...gameUpdate,latest_version:gameUpdate.installed_version,update_available:false,can_start:true,auth_required:false};setup.job={id:'repair-fixture',mode:'update',operation:'repair',state:'ready',phase:'ready',message:'Synthetic repair prepared.',checks:[],progress:100};status.game.can_start=true;res.end(JSON.stringify({ok:true,job:setup.job}));return;}
      if (url.pathname === '/api/game-update/check') {gameUpdate={...gameUpdate,latest_version:'1.9.0.0',update_available:true,can_start:true,auth_required:false};setup.job={id:'update-fixture',mode:'update',state:'ready',phase:'package_check',message:'Synthetic update available.',checks};status.game.can_start=true;res.end(JSON.stringify({ok:true,job:setup.job}));return;}
      if (url.pathname === '/api/game-update/start') {assert.equal(JSON.parse(body).check_id,setup.job.id);setup.job={...setup.job,state:'installing',phase:'download',can_pause:true,can_resume:false,progress:null};gameUpdate.can_start=false;status.game.can_start=false;res.end(JSON.stringify({ok:true,job:setup.job}));return;}
      if (url.pathname === '/api/game-update/rollback') {assert.deepEqual(JSON.parse(body),{});gameUpdate={...gameUpdate,installed_version:'1.8.16.0',can_rollback:false};res.end(JSON.stringify({ok:true}));return;}
      if (url.pathname === '/api/setup/pick') {res.end(JSON.stringify({ok:true,field:JSON.parse(body).field,cancelled:false,path:'/synthetic/native-picked'}));return;}
      if (url.pathname === '/api/setup/check') {const input=JSON.parse(body);setup.job={id:'check-1',mode:input.mode,market:input.market,game_id:input.game_id,runtime_path:nextCheckState==='ready'?(input.destination_path||input.runtime_path):undefined,state:nextCheckState,phase:'checked',message:'Synthetische Dateien geprüft.',progress:nextCheckState==='checking'?null:100,checks};setup.state=nextCheckState;status.game.can_start=nextCheckState==='failed';res.end(JSON.stringify({ok:true,job:setup.job}));return;}
      if (url.pathname === '/api/setup/start') {assert.equal(JSON.parse(body).check_id,setup.job.id);setup.job={...setup.job,state:setup.job.mode==='install'?'installing':'complete',phase:setup.job.mode==='install'?'authentication':'complete',progress:null,message:'Runtime verbunden.'};setup.state=setup.job.state;status.runtime.ready=true;status.runtime.configured=true;status.game.can_start=true;res.end(JSON.stringify({ok:true,job:setup.job}));return;}
      if (url.pathname === '/api/setup/pause') {assert.equal(JSON.parse(body).job_id,setup.job.id);setup.job={...setup.job,phase:'pausing',can_pause:false,can_resume:false};res.end(JSON.stringify({ok:true,job:setup.job}));return;}
      if (url.pathname === '/api/setup/resume') {assert.equal(JSON.parse(body).job_id,setup.job.id);setup.job={...setup.job,phase:'download',can_pause:true,can_resume:false};res.end(JSON.stringify({ok:true,job:setup.job}));return;}
      if (url.pathname === '/api/setup/cancel') {setup.job={...setup.job,state:'cancelled',message:'Abgebrochen.'};setup.state='cancelled';status.game.can_start=true;res.end(JSON.stringify({ok:true,job:setup.job}));return;}
      res.end(JSON.stringify({ok:true}));return;
    }
    res.writeHead(404);res.end('{}');return;
  }
  const name = url.pathname === '/' ? 'index.html' : url.pathname.slice(1);
  if (!files.has(name)) {res.writeHead(404);res.end();return;}
  const type = {html:'text/html',css:'text/css',js:'text/javascript',svg:'image/svg+xml',png:'image/png',woff2:'font/woff2'}[name.split('.').pop()];
  res.setHeader('Content-Type',type);res.end(await readFile(join(base,name)));
});
await new Promise((resolve,reject) => {server.once('error',reject);server.listen(0,'127.0.0.1',resolve);});
const origin = `http://127.0.0.1:${server.address().port}`;
const allowedOrigins = new Set([origin]);
const chrome = spawn(process.env.CHROME_BIN ?? process.env.CHROMIUM_BIN ?? process.env.CHROMIUM ?? 'chromium', [
  '--headless=new','--no-sandbox','--disable-gpu','--disable-dev-shm-usage','--no-first-run','--no-default-browser-check',
  '--disable-background-networking','--disable-component-update','--disable-sync','--disable-extensions',
  '--remote-debugging-port=0',`--user-data-dir=${join(temp,'profile')}`,'about:blank',
], {stdio:['ignore','ignore','pipe']});
let chromeErrors=''; chrome.stderr.on('data',chunk => {chromeErrors+=chunk;});
let chromeFailure=null; chrome.once('error',error=>{chromeFailure=error;});
let socket, realBackend;
const sleep = ms => new Promise(resolve => setTimeout(resolve,ms));
async function until(fn, message, timeout=10000) {
  const end=Date.now()+timeout;
  while(Date.now()<end) {if(await fn())return;await sleep(50);}
  throw new Error(message);
}
try {
  let port;
  await until(async()=>{
    if(chromeFailure)throw chromeFailure;
    if(chrome.exitCode!==null||chrome.signalCode!==null)throw new Error('Chromium exited before its debugger was ready');
    try{port=(await readFile(join(temp,'profile/DevToolsActivePort'),'utf8')).split('\n')[0];return !!port;}catch{return false;}
  }, 'Chromium did not start', 30000);
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  socket = new WebSocket(targets.find(t=>t.type==='page').webSocketDebuggerUrl);
  await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true});});
  let serial=0;const pending=new Map();
  socket.addEventListener('message',event=>{
    const data=JSON.parse(event.data);
    if(data.id){const entry=pending.get(data.id);pending.delete(data.id);if(data.error)entry.reject(new Error(JSON.stringify(data.error)));else entry.resolve(data.result);}
    if(data.method==='Runtime.exceptionThrown')errors.push(data.params.exceptionDetails.text);
    if(data.method==='Runtime.consoleAPICalled'&&['warning','error','assert'].includes(data.params.type))consoleIssues.push(data.params.type);
    if(data.method==='Network.requestWillBeSent' && ![...allowedOrigins].some(origin=>data.params.request.url.startsWith(origin+'/')) && !data.params.request.url.startsWith('blob:'))externalRequests.push(data.params.request.url);
  });
  const call=(method,params={})=>new Promise((resolve,reject)=>{const id=++serial;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params}));});
  const evaluate=async expression=>{
    const response=await call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});
    if(response.exceptionDetails)throw new Error(response.exceptionDetails.exception?.description ?? response.exceptionDetails.text);
    return response.result.value;
  };
  const check=async(name,expression)=>{assert.equal(await evaluate(expression),true,name);results.push(name);};
  const eventualCheck=async(name,expression)=>{await until(()=>evaluate(expression),name);results.push(name);};
  const click=async (id,{visible=true}={})=>{
    const target=`document.getElementById(${JSON.stringify(id)})`;
    await until(()=>evaluate(`!!${target} && !${target}.disabled && (!${visible} || ${target}.getClientRects().length>0)`),`${id} is not actionable`);
    await evaluate(`${target}.click()`);
  };
  const refresh=async expression=>{await click('refresh-status',{visible:false});await until(()=>evaluate(expression),'Refreshed state not rendered');};
  const language=async value=>{await evaluate(`document.getElementById('language-select').value=${JSON.stringify(value)};document.getElementById('language-select').dispatchEvent(new Event('change'))`);await until(()=>evaluate(`document.documentElement.lang===${JSON.stringify(value)}`),'Language not applied');};
  const route=async view=>{await evaluate(`location.hash=${JSON.stringify(view)}`);await until(()=>evaluate(`!document.getElementById('view-'+${JSON.stringify(view)}).hidden`),'Route not rendered');};
  const screenshot=async name=>{await sleep(200);const shot=await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});await writeFile(join(artifacts,name),Buffer.from(shot.data,'base64'));};
  // Page.navigate acknowledges before the replacement document has a root.
  // Poll absence as not-ready; do not turn that normal state into a TypeError.
  const queryLanguageReady=`document.documentElement?.lang==='de' && document.getElementById('game-state')?.textContent==='Bereit zum Start'`;
  await call('Runtime.enable');await call('Page.enable');await call('Network.enable');
  await evaluate('document.open()');
  await check('Navigation readiness waits safely while the new document has no root',`document.documentElement===null && (${queryLanguageReady})===false`);
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});
  await call('Page.navigate',{url:origin+'/?lang=de'});
  await until(()=>evaluate(`document.getElementById('game-state')?.textContent === 'Bereit zum Start'`),'Ready status absent');
  await until(()=>evaluate(`!document.getElementById('available-updates').hidden && document.getElementById('available-updates-label').textContent==='Updates für Flightdeck und MSFS sind verfügbar.'`),'Automatic startup update offers missing');
  await check('Startup checks show available updates on the real overview without blocking play',`location.origin===${JSON.stringify(origin)} && document.title==='Übersicht · Flightdeck' && !document.getElementById('launch-button').disabled && !document.getElementById('view-overview').hidden`);
  assert.equal(startupRequests.length,1);assert.equal(posts.length,0);results.push('Startup requests only version discovery, never download, login or install');
  await screenshot('startup-updates-de-desktop.png');
  await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await language('en');
  await check('Startup update notice is translated and fits mobile',`document.documentElement.scrollWidth<=innerWidth && document.getElementById('available-updates-label').textContent==='Updates for Flightdeck and MSFS are available.'`);
  await screenshot('startup-updates-en-mobile.png');
  await evaluate(`document.querySelector('#available-updates a').click()`);await until(()=>evaluate(`location.hash==='#updates' && document.getElementById('launcher-update-title').textContent==='Flightdeck 0.1.5 is available'`),'Startup notice did not open update actions');
  await check('Background MSFS discovery requires explicit preflight before download',`document.getElementById('update-start').hidden && !document.getElementById('update-check').disabled`);
  await refresh(`!document.getElementById('launch-button').disabled`);assert.equal(startupRequests.length,1);results.push('Language, navigation and status refresh do not repeat startup discovery');
  startupFixture=false;launcherUpdate=launcherDefault();gameUpdate={...gameUpdate,latest_version:null,update_available:null};
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});await language('de');await route('overview');
  await check('Save summary pluralizes nonzero counts',`document.getElementById('overview-save-detail').textContent.endsWith('3 Dateien · 1 Backup')`);
  status.saves.files=0;status.saves.backups=0;await refresh(`document.getElementById('save-files').textContent==='0' && document.getElementById('save-backups').textContent==='0'`);
  await check('Save summary pluralizes zero counts',`document.getElementById('overview-save-detail').textContent.endsWith('0 Dateien · 0 Backups')`);
  status.saves.files=1;status.saves.backups=2;await refresh(`document.getElementById('save-files').textContent==='1' && document.getElementById('save-backups').textContent==='2'`);
  await check('Save summary pluralizes single file',`document.getElementById('overview-save-detail').textContent.endsWith('1 Datei · 2 Backups')`);
  status.saves.files=3;status.saves.backups=1;await refresh(`document.getElementById('save-files').textContent==='3' && document.getElementById('save-backups').textContent==='1'`);
  await check('Ready launch enabled without the removed compatibility warning',`!document.getElementById('launch-button').disabled && !document.querySelector('.boundary-note')`);
  status.service={update_pending:true,message:'<b>Update bereit: laufende Sitzung abschließen.</b>'};
  await refresh(`!document.getElementById('service-notice').hidden`);
  await check('Deferred update is visible as plain text',`document.getElementById('service-notice').textContent===${JSON.stringify(status.service.message)} && !document.getElementById('service-notice').querySelector('b')`);
  status.service={update_pending:false,message:''};
  await refresh(`document.getElementById('service-notice').hidden`);
  await check('Deferred update clears without changing play permission',`!document.getElementById('launch-button').disabled`);
  await check('No CSRF rendered',`!document.documentElement.outerHTML.includes(${JSON.stringify(csrf)})`);
  await check('Desktop has no overflow',`document.documentElement.scrollWidth <= innerWidth`);
  await screenshot('overview-desktop.png');
  await check('Both game versions are directly visible with 2024 selected',`document.getElementById('version-msfs2024').getAttribute('aria-pressed')==='true' && !document.getElementById('version-msfs2020').disabled && getComputedStyle(document.querySelector('.launch-art-2024')).backgroundImage.includes('flight-panorama.png')`);
  const recordSwitch = async target => evaluate(`new Promise(resolve => {
    const frames = [], start = performance.now();
    const sample = () => {
      const panel = document.querySelector('.launch-panel').getBoundingClientRect();
      const button = document.getElementById('launch-button');
      frames.push({time:performance.now()-start, top:panel.top, height:panel.height,
        buttonTop:button.getBoundingClientRect().top, buttonOpacity:Number(getComputedStyle(button).opacity),
        indicator:document.getElementById('game-status-use').getAttribute('href'),
        notice:!document.getElementById('notice').hidden});
      if (performance.now()-start < 1300) requestAnimationFrame(sample); else resolve(frames);
    };
    sample();document.getElementById(${JSON.stringify(target)}).click();
  })`);
  switchDelay = 180;
  const switchFrames = await recordSwitch('version-msfs2020');
  switchDelay = 0;
  await writeFile(join(artifacts,'switch-frames.json'),JSON.stringify(switchFrames,null,2));
  assert.ok(switchFrames.length>10);
  for (const key of ['top','height','buttonTop']) {
    assert.ok(Math.max(...switchFrames.map(f=>f[key]))-Math.min(...switchFrames.map(f=>f[key]))<1,
      `Switch moves ${key} instead of keeping the layout stable`);
  }
  assert.ok(switchFrames.every(f=>f.buttonOpacity===1 && f.indicator==='#i-check-circle' && !f.notice),
    'Switch flashes the launch control, readiness indicator or success banner');
  results.push('Delayed game switch keeps every sampled hero/control position stable without dimming or banner shifts');
  await until(()=>evaluate(`document.getElementById('version-msfs2020').getAttribute('aria-pressed')==='true' && !document.getElementById('version-msfs2024').disabled`),'2020 switch did not render');
  assert.equal(posts.at(-1).path,'/api/game/select');assert.deepEqual(posts.at(-1).body,{game_id:'msfs2020'});assert.equal(posts.at(-1).token,csrf);
  await check('One click switches to 2020 with its own artwork and amber accent',`document.body.dataset.game==='msfs2020' && getComputedStyle(document.querySelector('.launch-art-2020')).backgroundImage.includes('flight-panorama-2020.png') && getComputedStyle(document.body).getPropertyValue('--accent').trim()==='#f5c873' && document.getElementById('launch-game-name').textContent==='Microsoft Flight Simulator 2020'`);
  await check('Game switch crossfades the prepared artwork',`getComputedStyle(document.querySelector('.launch-art-2020')).transitionProperty.includes('opacity') && getComputedStyle(document.querySelector('.launch-art-2020')).transitionDuration!=='0s' && document.querySelectorAll('.launch-art[aria-hidden=true]').length===2`);
  await until(()=>evaluate(`getComputedStyle(document.querySelector('.launch-art-2020')).opacity==='1' && document.querySelector('.launch-panel').getAnimations({subtree:true}).length===0`),'Game transition did not settle');
  await check('Only the selected title is visible and announced',`getComputedStyle(document.querySelector('.launch-title-2020')).opacity==='1' && getComputedStyle(document.querySelector('.launch-title-2024')).opacity==='0' && document.getElementById('launch-game-name').textContent==='Microsoft Flight Simulator 2020' && document.getElementById('version-announcement').textContent.includes('Simulator gewechselt')`);
  await click('version-msfs2024');
  await until(()=>evaluate(`document.body.dataset.game==='msfs2024' && Number(getComputedStyle(document.querySelector('.launch-art-2020')).opacity)<.9 && Number(getComputedStyle(document.querySelector('.launch-art-2020')).opacity)>.1`),'Reverse transition did not crossfade');
  await click('version-msfs2020');
  await until(()=>evaluate(`document.body.dataset.game==='msfs2020' && document.querySelector('.launch-panel').getAnimations({subtree:true}).length===0`),'Interrupted switch did not settle');
  await check('Reversing during the crossfade settles on one title without moving or dimming controls',`getComputedStyle(document.querySelector('.launch-title-2020')).opacity==='1' && getComputedStyle(document.querySelector('.launch-title-2024')).opacity==='0' && document.querySelector('.launch-content').getAnimations().length===0 && getComputedStyle(document.getElementById('launch-button')).opacity==='1' && document.getElementById('notice').hidden`);
  await refresh(`document.body.dataset.game==='msfs2020'`);
  await check('Unchanged polling preserves settled artwork and does not restart animation',`document.querySelector('.launch-content').getAnimations().length===0 && document.querySelector('.launch-art-2020').getAnimations().length===0`);
  await check('2020 installation checks identify the selected version',`document.getElementById('overview-checks').textContent.includes('MSFS 2020') && !document.getElementById('overview-checks').textContent.includes('MSFS 2024')`);
  await screenshot('overview-2020-desktop.png');
  await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
  await check('2020 switch and artwork fit mobile width',`document.documentElement.scrollWidth<=innerWidth && document.getElementById('version-msfs2020').getAttribute('aria-pressed')==='true' && getComputedStyle(document.querySelector('.launch-art-2020')).backgroundImage.includes('flight-panorama-2020.png')`);
  await screenshot('overview-2020-mobile.png');
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});
  await call('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:'reduce'}]});
  await click('version-msfs2024');
  await until(()=>evaluate(`document.getElementById('version-msfs2024').getAttribute('aria-pressed')==='true'`),'2024 switch did not render');
  await check('2024 restores its own artwork and accent',`document.body.dataset.game==='msfs2024' && getComputedStyle(document.querySelector('.launch-art-2024')).backgroundImage.includes('flight-panorama.png') && getComputedStyle(document.body).getPropertyValue('--accent').trim()==='#64e7f2'`);
  await check('Reduced motion switches immediately without title or background movement',`getComputedStyle(document.querySelector('.launch-art-2020')).opacity==='0' && getComputedStyle(document.querySelector('.launch-title-2024')).opacity==='1' && getComputedStyle(document.querySelector('.launch-title-2020')).opacity==='0' && document.querySelector('.launch-panel').getAnimations({subtree:true}).length===0`);
  const beforeFailure=await evaluate(`document.querySelector('.launch-panel').getBoundingClientRect().top`);
  failNext=true;
  await click('version-msfs2020');
  await until(()=>evaluate(`!document.getElementById('notice').hidden && !document.getElementById('version-msfs2020').disabled`),'Failed switch did not show feedback');
  await check('Failed switch preserves the confirmed game and reports the error without shifting the layout',`document.body.dataset.game==='msfs2024' && document.getElementById('version-msfs2024').getAttribute('aria-pressed')==='true' && document.getElementById('notice').classList.contains('error') && !document.getElementById('notice').querySelector('img') && document.querySelector('.launch-panel').getBoundingClientRect().top===${beforeFailure}`);
  await call('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:'no-preference'}]});
  await click('launch-button');
  await until(()=>evaluate(`document.getElementById('game-state').textContent === 'Simulator läuft' && !document.getElementById('launch-button').disabled`),'Launch transition failed');
  assert.equal(posts.at(-1).path,'/api/launch');assert.equal(posts.at(-1).token,csrf);results.push('Launch sends exact local JSON + CSRF and reflects backend');
  await check('Running disables runtime configuration and game switching',`document.getElementById('config-button').disabled && document.getElementById('version-msfs2020').disabled`);
  await click('launch-button');
  await until(()=>evaluate(`document.getElementById('game-state').textContent === 'Bereit zum Start' && !document.getElementById('launch-button').disabled`),'Stop transition failed');
  assert.equal(posts.at(-1).path,'/api/stop');results.push('Stop uses managed session API');
  await check('Action feedback has an accessible close control',`!document.getElementById('notice').hidden && document.querySelector('#notice button').getAttribute('aria-label')==='Hinweis schließen'`);
  await evaluate(`document.querySelector('#notice button').click()`);
  await check('Notice can be dismissed immediately',`document.getElementById('notice').hidden`);
  automaticFixture=true;
  const autoBegin=posts.length;
  await click('launch-button');await until(()=>evaluate(`document.getElementById('launch-label').textContent==='Start wird vorbereitet …'`),'Automatic prelaunch did not render');
  await check('Normal Play waits for automatic sync without a second launch button',`document.getElementById('launch-button').disabled && !document.getElementById('overview-cloud-progress').hidden && !document.getElementById('overview-cloud-cancel-auto').disabled && document.getElementById('config-button').disabled && document.getElementById('backup-button').disabled`);
  await click('overview-cloud-cancel-auto');await until(()=>evaluate(`!document.getElementById('launch-button').disabled`),'Dedicated prelaunch cancellation did not finish');
  assert.deepEqual(posts.at(-1).body,{request_id:autoRequest});assert.equal(posts.at(-1).path,'/api/cloud-saves/cancel-auto');results.push('Prelaunch cancel uses only the current automatic request');
  await click('launch-button');await until(()=>status.cloud.state==='syncing','Second prelaunch missing');
  status.cloud={...autoIdle(),state:'playing',phase:'before_start',request_id:autoRequest};status.game={state:'running',managed:true,can_start:false,can_stop:true};
  await refresh(`document.getElementById('launch-label').textContent==='Simulator beenden' && !document.getElementById('launch-button').disabled`);
  await check('The same Play action becomes Stop despite automatic lifecycle ownership',`!document.getElementById('launch-button').disabled && document.getElementById('overview-cloud-cancel-auto').hidden`);
  assert.equal(posts.slice(autoBegin).filter(p=>p.path==='/api/launch').length,2);
  await click('launch-button');await until(()=>evaluate(`document.getElementById('launch-label').textContent==='Spielstände werden gesichert …'`),'Postexit sync missing');
  await check('After exit the launcher waits for upload and offers no unsafe local launch',`document.getElementById('launch-button').disabled && !document.getElementById('overview-cloud-progress').hidden && document.getElementById('overview-cloud-play-local').hidden`);
  status.cloud={...autoIdle(),state:'synced',last_synced_at:'2026-09-18T12:00:00Z'};status.game.can_start=true;
  await refresh(`document.getElementById('overview-cloud-title').textContent==='Spielstände synchronisiert' && !document.getElementById('launch-button').disabled`);
  await check('Confirmed sync shows its real timestamp',`document.getElementById('overview-cloud-time').textContent.includes('18.09.2026')`);
  status.cloud={...autoIdle(),state:'attention',phase:'before_start',request_id:autoRequest,can_retry:true,can_play_local:true,error_code:'authentication',message:'<img src=x onerror="window.injected=1"> Synthetic auth failure.'};status.game.can_start=false;
  await refresh(`!document.getElementById('overview-cloud-retry').disabled`);
  await check('Offline or sign-in attention offers retry and explicit local play as text only',`!document.getElementById('overview-cloud-play-local').disabled && document.getElementById('launch-button').disabled && !document.querySelector('#overview-cloud-message img') && !window.injected`);
  const autoReloadPosts=posts.length;await language('en');await call('Page.reload');await until(()=>evaluate(`document.getElementById('overview-cloud-retry')?.textContent==='Retry sync' && !document.getElementById('overview-cloud-retry').disabled`),'Automatic attention did not survive reload');
  assert.equal(posts.length,autoReloadPosts);results.push('Reload and language change never retry, resolve or start automatically');
  await click('overview-cloud-retry');await until(()=>status.cloud.state==='syncing','Retry not sent');assert.equal(posts.at(-1).path,'/api/cloud-saves/retry');assert.deepEqual(posts.at(-1).body,{request_id:autoRequest});
  status.cloud={...autoIdle(),state:'attention',phase:'before_start',request_id:autoRequest,can_retry:true,can_play_local:true,error_code:'authentication'};await refresh(`!document.getElementById('overview-cloud-play-local').disabled`);
  await click('overview-cloud-play-local');await until(()=>evaluate(`document.getElementById('launch-label').textContent==='Stop simulator' && !document.getElementById('launch-button').disabled`),'Explicit local play did not start');assert.deepEqual(posts.at(-1).body,{request_id:autoRequest});results.push('Explicit local play retains the managed Stop action');
  status.game={state:'stopped',managed:false,can_start:false,can_stop:false};
  const conflict=()=>({...autoIdle(),state:'attention',phase:'before_start',request_id:autoRequest,conflict:true,can_retry:true,can_play_local:true,summary:{container_count:2,local_container_count:3,conflict_count:1}});
  status.cloud=conflict();await refresh(`!document.getElementById('overview-cloud-cloud').disabled`);
  await check('Conflicts offer both versions and counts without guessing a winner',`!document.getElementById('overview-cloud-conflict').hidden && !document.getElementById('overview-cloud-local').disabled && document.getElementById('overview-cloud-play-local').hidden && document.getElementById('overview-cloud-counts').textContent==='2 save groups in the cloud · 3 on this computer'`);
  await screenshot('automatic-conflict-en-desktop.png');await click('overview-cloud-cloud');await until(()=>posts.at(-1)?.path==='/api/cloud-saves/resolve','Cloud conflict choice missing');assert.deepEqual(posts.at(-1).body,{request_id:autoRequest,choice:'cloud'});
  status.cloud=conflict();await refresh(`!document.getElementById('overview-cloud-local').disabled`);await route('saves');
  await check('Saves starts with automatic controls and closed advanced tools',`!document.getElementById('cloud-advanced').open && !document.getElementById('auto-cloud-local').disabled && !document.getElementById('cloud-check').checkVisibility()`);
  await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await check('Automatic conflict actions fit the mobile viewport',`document.documentElement.scrollWidth<=innerWidth`);await screenshot('automatic-conflict-en-mobile.png');
  await click('auto-cloud-local');await until(()=>posts.at(-1)?.body?.choice==='local','Local conflict choice missing');assert.deepEqual(posts.at(-1).body,{request_id:autoRequest,choice:'local'});assert.equal(posts.at(-1).token,csrf);results.push('Both conflict choices use exact request identity and CSRF');
  status.cloud={...autoIdle(),state:'attention',phase:'after_exit',request_id:autoRequest,can_retry:true,error_code:'network'};await refresh(`!document.getElementById('auto-cloud-retry').disabled`);
  await check('Postexit attention without local capability offers no local start',`document.getElementById('auto-cloud-title').textContent==='Cloud sync needs attention' && document.getElementById('auto-cloud-play-local').hidden && document.getElementById('launch-button').disabled`);
  status.cloud={...status.cloud,error_code:'transport',can_play_local:true,message:'Your saves are backed up locally. Retry the cloud upload or continue with local saves. The pending sync is preserved.'};
  await refresh(`!document.getElementById('auto-cloud-play-local').disabled && !document.getElementById('auto-cloud-play-local').hidden`);
  await check('Recoverable postexit failure offers explicit local continuation on mobile',`document.documentElement.scrollWidth<=innerWidth && document.getElementById('launch-button').disabled && document.getElementById('auto-cloud-title').textContent==='Cloud sync needs attention'`);
  await evaluate(`document.getElementById('auto-cloud-title').scrollIntoView({block:'start'})`);await screenshot('automatic-upload-recovery-en-mobile.png');
  const localContinuePosts=posts.length;
  await click('auto-cloud-play-local');await until(()=>evaluate(`document.getElementById('launch-label').textContent==='Stop simulator' && !document.getElementById('launch-button').disabled`),'Postexit local continuation did not start');
  assert.deepEqual(posts.slice(localContinuePosts).map(p=>({path:p.path,body:p.body})),[{path:'/api/cloud-saves/play-local',body:{request_id:autoRequest}}]);results.push('Postexit local continuation sends exactly one managed local-play request');
  status.game={state:'stopped',managed:false,can_start:false,can_stop:false};status.cloud={...autoIdle(),state:'attention',phase:'after_exit',request_id:autoRequest,can_retry:true,can_play_local:true,error_code:'transport'};
  await refresh(`!document.getElementById('auto-cloud-retry').disabled`);
  const beforeStaleAuto=posts.length;
  statusBarrier=new Promise(resolve=>releaseStatus=resolve);statusWaiting=false;
  await click('auto-cloud-retry');await until(()=>statusWaiting,'Automatic action did not request current status');
  status.cloud={...status.cloud,request_id:'223456781234423482341234567890ab'};
  releaseStatus();statusBarrier=null;
  await until(()=>evaluate(`!document.getElementById('auto-cloud-retry').disabled`),'Automatic stale action left controls locked');
  assert.equal(posts.length,beforeStaleAuto);results.push('A replaced automatic request is never resolved or retried from a stale click');
  automaticFixture=false;status.cloud=autoIdle();status.game.can_start=true;
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});await language('de');await route('overview');await refresh(`!document.getElementById('launch-button').disabled`);
  status.game={state:'external',managed:false,can_start:false,can_stop:true};await refresh(`document.getElementById('game-state').textContent.includes('außerhalb')`);
  await check('External session cannot stop',`document.getElementById('launch-button').disabled && document.getElementById('game-state').textContent.includes('außerhalb')`);
  status.game={state:'stopped',managed:false,can_start:true,can_stop:false};status.runtime.ready=false;status.runtime.configured=false;await refresh(`document.getElementById('game-state').textContent==='Installation noch nicht verbunden'`);
  await click('launch-button');await check('Missing runtime navigates to installation',`location.hash === '#installation' && !document.getElementById('view-installation').hidden`);
  await evaluate(`document.querySelector('input[name=setup_mode][value=install]').click()`);
  await check('Missing install capability disables download honestly',`document.getElementById('config-button').disabled && !document.getElementById('install-unavailable').hidden`);
  setup.install_available=true;await route('overview');await route('installation');
  await until(()=>evaluate(`!document.getElementById('config-button').disabled`),'Install capability not applied');
  await check('Install enables cloud sync and local backups by default',`!document.getElementById('setup-install-fields').hidden && !!document.getElementById('install-save-policy') && !document.getElementById('install-local-saves') && !document.getElementById('setup-advanced').open`);
  await check('Suggested region is a visible native dropdown outside collapsed folder settings',`document.getElementById('install-destination-summary').textContent==='/synthetic/new-msfs' && document.getElementById('install-market').tagName==='SELECT' && document.getElementById('install-market').selectedOptions[0].textContent==='Österreich (AT)' && !document.getElementById('install-market').closest('details') && !document.getElementById('install-location').open && document.getElementById('install-market').getBoundingClientRect().height>0`);
  setup.directory_picker=true;await route('overview');await route('installation');
  await until(()=>evaluate(`!document.getElementById('install-destination-pick').hidden`),'Install folder button did not appear');
  await evaluate(`document.getElementById('install-destination').value='/synthetic/msfs2024';document.getElementById('setup-game-id').value='msfs2020';document.getElementById('setup-game-id').dispatchEvent(new Event('change'))`);
  await check('Changing the edition moves the suggested destination to its own folder',`document.getElementById('install-destination').value==='/synthetic/msfs2020'`);
  const beforeInstallPick=posts.length;await click('install-destination-pick');
  await until(()=>evaluate(`document.getElementById('install-destination').value==='/synthetic/native-picked/msfs2020'`),'New edition destination was not chosen');
  assert.equal(posts.at(-1).path,'/api/setup/pick');assert.deepEqual(posts.at(-1).body,{field:'destination_path',initial:'/synthetic'});
  assert.equal(posts.length,beforeInstallPick+1);results.push('Folder picker selects a parent and creates an edition-specific new destination');
  nextCheckState='ready';await click('config-button');
  await until(()=>evaluate(`!document.getElementById('setup-start').disabled`),'MSFS 2020 preflight did not become ready');
  await check('MSFS 2020 ready step shows its exact new folder apart from connected MSFS 2024',`document.getElementById('setup-target-path').textContent==='/synthetic/native-picked/msfs2020' && document.getElementById('setup-target-label').textContent.includes('MSFS 2020') && document.getElementById('current-path').textContent==='/opt/flightdeck-fixture/MSFS2024'`);
  assert.equal(posts.at(-1).body.game_id,'msfs2020');assert.equal(posts.at(-1).body.destination_path,'/synthetic/native-picked/msfs2020');
  await click('setup-cancel');await until(()=>evaluate(`!document.getElementById('setup-reset').hidden`),'MSFS 2020 ready job did not cancel');await click('setup-reset');setup.job=null;setup.state='idle';
  await evaluate(`document.getElementById('setup-game-id').value='msfs2024';document.getElementById('setup-game-id').dispatchEvent(new Event('change'));document.getElementById('install-destination').value='/synthetic/new-msfs';document.getElementById('install-destination').dispatchEvent(new Event('input'))`);
  const beforeRegionSelection=posts.length;
  await evaluate(`document.getElementById('install-market').focus()`);
  await call('Input.dispatchKeyEvent',{type:'keyDown',key:'ArrowDown',code:'ArrowDown',windowsVirtualKeyCode:40});
  await call('Input.dispatchKeyEvent',{type:'keyUp',key:'ArrowDown',code:'ArrowDown',windowsVirtualKeyCode:40});
  await check('Region dropdown responds to the keyboard without starting setup',`document.getElementById('install-market').value!=='AT' && document.activeElement===document.getElementById('install-market')`);
  await evaluate(`document.getElementById('install-market').value='DE';document.getElementById('install-market').dispatchEvent(new Event('change',{bubbles:true}))`);
  await language('en');await route('overview');await route('installation');
  await check('Language switch and setup polls preserve an explicit region and translate its name',`document.getElementById('install-market').value==='DE' && document.getElementById('install-market').selectedOptions[0].textContent==='Germany (DE)' && document.getElementById('install-destination').value==='/synthetic/new-msfs'`);
  assert.equal(posts.length,beforeRegionSelection);
  setup.defaults.market='';await evaluate(`window.regionReloadPending=true`);await call('Page.reload');
  await until(()=>evaluate(`!window.regionReloadPending && document.getElementById('runtime-path')?.value===${JSON.stringify(setup.defaults.runtime_path)} && !document.querySelector('input[value=install]').disabled`),'Setup did not reload for unknown-region check');
  await evaluate(`document.querySelector('input[value=install]').click()`);
  await check('Unknown region stays unselected and prevents install instead of silently using US',`document.getElementById('install-market').value==='' && document.getElementById('install-market').selectedOptions[0].textContent==='Choose a region …' && document.getElementById('config-button').disabled`);
  assert.equal(posts.length,beforeRegionSelection);
  setup.defaults.market='AT';
  await evaluate(`document.getElementById('install-market').value='AT';document.getElementById('install-market').dispatchEvent(new Event('change',{bubbles:true}));document.getElementById('setup-market').value='AT';document.getElementById('setup-market').dispatchEvent(new Event('change',{bubbles:true}))`);
  await language('de');
  setup.prepare_available=false;setup.prepare_unavailable_reason='Advanced build tools missing';await route('overview');await route('installation');
  await until(()=>evaluate(`document.querySelector('input[value=prepare]').disabled`),'Advanced capability not applied');
  await check('Missing advanced build tools do not warn or block normal installation',`document.getElementById('setup-unavailable').hidden && !document.getElementById('config-button').disabled`);
  setup.prepare_available=true;setup.prepare_unavailable_reason='';
  await evaluate(`document.getElementById('notice').hidden=true`);
  await screenshot('setup-install-desktop.png');
  await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
  await check('Primary install setup fits the mobile viewport with a visible region dropdown',`document.documentElement.scrollWidth<=innerWidth && document.getElementById('install-market').getBoundingClientRect().width<=innerWidth && document.getElementById('install-market').getClientRects().length>0 && !document.getElementById('install-market').closest('details')`);await screenshot('setup-install-mobile.png');
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});
  nextCheckState='failed';const beforeFailedCheck=posts.length;await click('config-button');
  await until(()=>evaluate(`!document.getElementById('setup-reset').hidden && !document.getElementById('setup-reset').disabled`),'Preflight failure did not render');
  setup.job={...setup.job,failure_phase:'paths',error:'Synthetic missing Linux component.',message:'Synthetic missing Linux component.'};
  await until(()=>evaluate(`document.getElementById('setup-job-message').hidden && document.getElementById('setup-error').textContent==='Synthetic missing Linux component.'`),'Failure detail repeated or missing');
  await check('Preflight failure explains next step without automatically starting login',`document.getElementById('setup-next-step').textContent.includes('Softwareverwaltung') && !document.getElementById('setup-install-help').hidden && document.getElementById('setup-start').hidden`);
  assert.deepEqual(posts.slice(beforeFailedCheck).map(post=>post.path),['/api/setup/check']);
  await screenshot('setup-requirements-error-desktop.png');
  await click('setup-reset');nextCheckState='ready';
  await check('Retry returns to unchanged settings without issuing another request',`!document.getElementById('runtime-form').hidden && document.getElementById('install-destination').value==='/synthetic/new-msfs' && document.getElementById('install-market').value==='AT' && !!document.getElementById('install-save-policy') && !document.getElementById('install-local-saves')`);
  assert.equal(posts.length,beforeFailedCheck+1);
  await evaluate(`document.getElementById('install-market').value='DE';document.getElementById('install-market').dispatchEvent(new Event('change',{bubbles:true}))`);
  const installPostStart=posts.length;
  await click('config-button');await until(()=>evaluate(`!document.getElementById('setup-start').disabled && document.getElementById('setup-start').getClientRects().length>0`),'Install preflight not ready');
  await check('Ready install shows its new target and labels the old connected runtime separately',`document.getElementById('setup-target-path').textContent==='/synthetic/new-msfs' && document.getElementById('setup-target-label').textContent.includes('MSFS 2024') && document.querySelector('.setup-current').textContent.includes('Aktuell verbunden:') && document.getElementById('current-path').textContent==='/opt/flightdeck-fixture/MSFS2024'`);
  assert.equal(posts.at(-1).body.destination_path,'/synthetic/new-msfs');
  assert.deepEqual(posts.slice(installPostStart).map(post=>post.path),['/api/setup/check']);
  assert.deepEqual(posts.at(-1).body,{mode:'install',market:'DE',local_saves:true,game_id:'msfs2024',destination_path:'/synthetic/new-msfs'});results.push('Install preflight sends the explicitly selected edition, region and actual location without starting login');
  await evaluate(`window.regionReloadPending=true`);await call('Page.reload');
  await until(()=>evaluate(`!window.regionReloadPending && document.getElementById('setup-start')?.getClientRects().length>0 && !document.getElementById('setup-start').disabled`),'Ready plan did not return after reload');
  await check('Reloaded ready plan keeps its actual region instead of the suggested default',`document.getElementById('install-market').value==='DE' && document.getElementById('install-market').disabled && document.getElementById('install-market').selectedOptions[0].textContent==='Deutschland (DE)'`);
  assert.equal(posts.length,installPostStart+1);
  await check('Login and licensed download require a separate explicit action',`document.getElementById('setup-start').textContent==='Anmelden & herunterladen'`);
  await click('setup-start');await until(()=>evaluate(`document.getElementById('setup-job-title').textContent==='Microsoft-Anmeldung'`),'Authentication phase missing');
  await check('Authentication phase reports progress without invented percentages',`!document.getElementById('setup-progress').hidden && !document.getElementById('setup-progress').hasAttribute('value')`);
  await check('Authentication explains the separate real account window and cannot pause',`document.getElementById('setup-next-step').textContent.includes('öffnet sich separat') && document.getElementById('setup-pause').hidden`);
  const transferFixture={kind:'components',received_bytes:512_000_000,verified_bytes:400_000_000,total_bytes:2_000_000_000,completed_files:1,total_files:3};
  setup.job={...setup.job,phase:'bootstrap',transfer:transferFixture};
  await eventualCheck('Component download uses its actual byte count rather than phase progress',`document.getElementById('setup-transfer').textContent==='512 MB von 2 GB · 25,6 %' && document.getElementById('setup-progress').value===25.6`);
  setup.job={...setup.job,phase:'download'};
  await until(()=>evaluate(`document.getElementById('setup-job-title').textContent==='MSFS wird heruntergeladen'`),'Download phase missing');
  await check('Download phase follows the backend and remains indeterminate',`!document.getElementById('setup-progress').hasAttribute('value')`);
  await check('Older download backends do not expose unsupported pause',`document.getElementById('setup-pause').hidden && document.getElementById('setup-resume').hidden`);
  setup.job={...setup.job,transfer:{...transferFixture,kind:'game',total_bytes:null}};
  await eventualCheck('Unknown download size shows only received bytes with indeterminate progress',`document.getElementById('setup-transfer').textContent==='512 MB empfangen' && !document.getElementById('setup-progress').hasAttribute('value')`);
  setup.job={...setup.job,can_pause:true,transfer:{...transferFixture,kind:'game'}};
  await until(()=>evaluate(`!document.getElementById('setup-pause').hidden && !document.getElementById('setup-pause').disabled`),'Pause capability not applied');
  await eventualCheck('Game download shows exact DE byte progress',`document.getElementById('setup-transfer').textContent==='512 MB von 2 GB · 25,6 %' && document.getElementById('setup-progress').value===25.6`);
  await evaluate(`window.transferBar=document.getElementById('setup-progress');window.transferMutations=0;window.transferObserver=new MutationObserver(events=>{window.transferMutations+=events.length});transferObserver.observe(document.getElementById('setup-transfer'),{childList:true,characterData:true,subtree:true});transferObserver.observe(transferBar,{attributes:true,attributeFilter:['value']});document.getElementById('setup-pause').focus()`);
  const transferReads=apiRequests.filter(r=>r.path==='/api/setup').length;
  await until(()=>apiRequests.filter(r=>r.path==='/api/setup').length>transferReads+1,'Background setup polling missing');
  await check('Unchanged transfer polling preserves progress nodes, values and focused action',`transferBar===document.getElementById('setup-progress') && transferMutations===0 && document.activeElement.id==='setup-pause'`);
  await evaluate(`transferObserver.disconnect()`);await screenshot('setup-download-de-desktop.png');
  const beforePause=posts.length;await click('setup-pause');
  await until(()=>evaluate(`document.getElementById('setup-job-title').textContent==='Download wird pausiert'`),'Pausing transition missing');
  await check('Pause transition cannot be clicked twice and keeps launch blocked',`document.getElementById('setup-pause').disabled && document.getElementById('setup-resume').hidden && document.getElementById('launch-button').disabled`);
  setup.job={...setup.job,phase:'paused',can_resume:true};
  await until(()=>evaluate(`!document.getElementById('setup-resume').hidden && !document.getElementById('setup-resume').disabled`),'Paused result missing');
  await check('Paused download keeps a stationary byte bar and describes session-only continuation',`!document.getElementById('setup-progress').hidden && document.getElementById('setup-progress').value===25.6 && document.getElementById('setup-transfer').textContent==='Pausiert · 512 MB von 2 GB · 25,6 %' && document.getElementById('setup-next-step').textContent.includes('Lass Flightdeck geöffnet') && document.getElementById('launch-button').disabled && !document.getElementById('setup-cancel').hidden`);
  await language('en');await check('Pause controls and limits translate without starting a download',`document.getElementById('setup-job-title').textContent==='Download paused' && document.getElementById('setup-resume').textContent==='Resume download' && document.getElementById('setup-next-step').textContent.includes('Keep Flightdeck open')`);
  await check('Paused received bytes and percentage translate without changing quantity',`document.getElementById('setup-transfer').textContent==='Paused · 512 MB of 2 GB · 25.6 %'`);
  await screenshot('setup-paused-en-desktop.png');
  await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await check('Paused download actions fit on mobile without scrolling',`document.documentElement.scrollWidth<=innerWidth && document.getElementById('setup-resume').getBoundingClientRect().bottom<=innerHeight && document.getElementById('setup-mode-choices').hidden`);await screenshot('setup-paused-en-mobile.png');
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});
  await call('Page.reload');await until(()=>evaluate(`document.getElementById('setup-job-title').textContent==='Download paused'`),'Paused job was not restored after page reload');
  assert.deepEqual(posts.slice(beforePause).map(post=>post.path),['/api/setup/pause']);results.push('Language change and page reload never resume a paused download automatically');
  await click('setup-resume');await until(()=>evaluate(`document.getElementById('setup-job-title').textContent==='Downloading MSFS'`),'Download not resumed');
  assert.deepEqual(posts.slice(beforePause).map(post=>post.path),['/api/setup/pause','/api/setup/resume']);assert.equal(posts.at(-1).body.job_id,setup.job.id);assert.equal(posts.at(-1).token,csrf);results.push('Explicit resume uses the exact existing job and CSRF');
  await language('de');
  await call('Page.reload');
  await until(()=>evaluate(`document.getElementById('setup-job-title').textContent==='MSFS wird heruntergeladen'`),'Active install mode was not restored on reload');
  await check('Reload restores the active install job and its actual transfer',`document.querySelector('input[name=setup_mode][value=install]').checked && document.getElementById('setup-progress').value===25.6 && document.getElementById('setup-transfer').textContent==='512 MB von 2 GB · 25,6 %'`);
  setup.job={...setup.job,phase:'provision'};
  await eventualCheck('Provisioning clears download bytes without claiming setup is complete',`document.getElementById('setup-transfer').hidden && !document.getElementById('setup-progress').hasAttribute('value') && document.getElementById('setup-complete').hidden`);
  await click('setup-cancel');await click('setup-reset');
  await evaluate(`document.querySelector('input[name=setup_mode][value=existing]').click()`);
  await until(()=>evaluate(`document.querySelectorAll('#discovery-results input').length===1`),'Discovered installation missing');
  await check('First setup screen hides technical preparation',`!document.getElementById('setup-advanced').open && document.getElementById('setup-prepare-fields').getClientRects().length===0`);
  const setupPostStart=posts.length;
  await evaluate(`document.querySelector('#discovery-results input').click()`);
  await check('Discovered installation selection copies its actual path without mutation',`document.getElementById('runtime-path').value==='/synthetic/path with spaces'`);
  assert.equal(posts.length,setupPostStart);
  await screenshot('setup-discovered-desktop.png');
  await click('config-button');
  await until(()=>evaluate(`!document.getElementById('setup-complete').hidden && !document.querySelector('input[name=setup_mode]').disabled`),'Setup completion missing');
  assert.deepEqual(posts.slice(setupPostStart).map(post=>post.path),['/api/setup/check','/api/setup/start']);
  assert.equal(posts[setupPostStart].body.runtime_path,'/synthetic/path with spaces');results.push('Connect validates and commits only the selected existing installation');
  assert.equal(posts.at(-1).body.check_id,setup.job.id);results.push('Existing connection commit uses the exact successful check ID');
  setup.job=null;setup.state='idle';setup.directory_picker=true;
  await route('overview');await route('installation');
  await until(()=>evaluate(`!document.getElementById('runtime-form').hidden`),'Setup inputs not restored');
  await check('Exactly one navigation entry is active after route change',`document.querySelectorAll('.navigation [aria-current="page"]').length===1 && document.querySelector('.navigation [aria-current="page"]').dataset.nav==='installation' && getComputedStyle(document.querySelector('[data-nav="overview"]')).borderLeftColor !== getComputedStyle(document.querySelector('[data-nav="installation"]')).borderLeftColor`);
  discovered=[];await click('setup-discover');
  await until(()=>evaluate(`document.getElementById('discovery-message').textContent.includes('Keine vorbereitete Installation')`),'Empty discovery fallback missing');
  await check('No discovery matches offer a real folder picker',`document.getElementById('setup-manual').open && !document.getElementById('runtime-path').disabled`);
  await screenshot('setup-existing-desktop.png');
  await check('Native picker only appears when backend supports it',`[...document.querySelectorAll('.path-picker button')].some(b=>!b.hidden)`);
  await until(()=>evaluate(`!document.querySelector('.path-picker button').disabled`),'Picker not actionable');
  await evaluate(`document.querySelector('.path-picker button').click()`);
  await until(()=>evaluate(`document.getElementById('runtime-path').value==='/synthetic/native-picked' && !document.querySelector('input[name=setup_mode][value=prepare]').disabled`),'Native picker result not applied');
  assert.equal(posts.at(-1).path,'/api/setup/pick');results.push('Explicit native directory picker uses CSRF and copies only its selected field');
  await evaluate(`document.querySelector('#setup-advanced>summary').click()`);
  await until(()=>evaluate(`document.getElementById('setup-advanced').open`),'Advanced setup did not open');
  await evaluate(`document.querySelector('input[name=setup_mode][value=prepare]').click()`);
  await check('New runtime enables cloud sync and offers the same localized region dropdown',`!!document.getElementById('setup-save-policy') && !document.getElementById('setup-local-saves') && !document.getElementById('setup-prepare-fields').hidden && document.getElementById('setup-market').tagName==='SELECT' && document.getElementById('setup-market').selectedOptions[0].textContent==='Österreich (AT)'`);
  await evaluate(`for(const id of ['setup-artifacts','setup-game','setup-runner','setup-prefix','setup-destination'])document.getElementById(id).value='/synthetic/'+id;`);
  await screenshot('setup-prepare-desktop.png');
  const commitsBeforePrepare=posts.filter(post=>post.path==='/api/setup/start').length;
  await click('config-button');await until(()=>evaluate(`!document.getElementById('setup-start').disabled && document.getElementById('setup-start').getClientRects().length>0`),'Advanced setup was not checked');
  await check('Advanced preparation still needs explicit commit and locks launch',`!document.getElementById('setup-start').hidden && document.getElementById('launch-button').disabled`);
  assert.equal(posts.filter(post=>post.path==='/api/setup/start').length,commitsBeforePrepare);
  await screenshot('setup-ready-desktop.png');
  await click('setup-cancel');await click('setup-reset');
  nextCheckState='checking';await click('config-button');
  await until(()=>evaluate(`!document.getElementById('setup-progress').hidden`),'Checking progress missing');
  await check('Unknown copy progress is indeterminate',`!document.getElementById('setup-progress').hasAttribute('value')`);
  assert.equal(posts.at(-1).body.mode,'prepare');assert.equal(posts.at(-1).body.local_saves,true);assert.equal(posts.at(-1).body.game_path,'/synthetic/setup-game');results.push('Preparation sends exact component paths and enables the required local save provider');
  const beforeLanguagePosts=posts.length;
  await language('en');
  await until(()=>evaluate(`document.getElementById('setup-job-message').textContent==='Synthetic files checked.'`),'Active setup job was not retranslated');
  await check('Language switch translates active setup without changing paths or automatic save policy',`document.getElementById('setup-job-title').textContent==='Checking installation' && document.getElementById('setup-game').value==='/synthetic/setup-game' && !!document.getElementById('setup-save-policy') && !document.getElementById('setup-local-saves') && document.getElementById('setup-game').disabled && document.getElementById('setup-checks').textContent.includes('Game installation')`);
  assert.equal(posts.length,beforeLanguagePosts);assert.ok(apiRequests.some(r=>r.path==='/api/setup'&&r.language==='en'));results.push('Language switch refetches with Accept-Language and never repeats setup mutation');
  await check('English labels, ARIA and placeholders use the same language',`document.querySelector('[data-nav=installation]').textContent==='Setup' && document.getElementById('language-select').getAttribute('aria-label')==='Language' && document.getElementById('runtime-path').placeholder==='/path/to/msfs-runtime'`);
  await screenshot('setup-checking-en-desktop.png');
  await language('de');
  await until(()=>evaluate(`document.getElementById('setup-job-message').textContent==='Synthetische Dateien geprüft.'`),'German setup job was not restored');
  // Hold the refresh after Cancel: the response renders first, but controls must
  // remain disabled until reconciliation finishes (the hosted-CI race).
  await until(()=>evaluate(`!document.getElementById('setup-cancel').disabled`),'Checking setup cannot be cancelled');
  const launchCount=posts.filter(post=>post.path==='/api/launch').length;
  statusBarrier=new Promise(resolve=>{releaseStatus=resolve;});
  await click('setup-cancel');await until(async()=>statusWaiting && await evaluate(`!document.getElementById('setup-reset').hidden`),'Cancellation state missing');
  await check('Cancelled setup remains locked during pending status reconciliation',`document.getElementById('setup-reset').disabled && document.getElementById('launch-button').disabled`);
  releaseStatus();statusBarrier=null;releaseStatus=null;
  assert.equal(posts.at(-1).path,'/api/setup/cancel');results.push('Setup cancellation uses exact job ID and returns to editable inputs');
  await click('setup-reset');await check('Cancelled setup is editable and does not launch game',`!document.getElementById('runtime-form').hidden && !document.getElementById('setup-game').disabled`);
  assert.equal(posts.filter(post=>post.path==='/api/launch').length,launchCount);
  nextCheckState='failed';await click('config-button');await until(()=>evaluate(`document.getElementById('setup-job-title').textContent==='Einrichtung nicht abgeschlossen'`),'Setup failure state missing');
  await check('Failed setup never enables commit',`document.getElementById('setup-start').hidden`);
  await click('setup-reset');nextCheckState='ready';
  status.runtime.ready=true;status.runtime.configured=true;await refresh(`document.getElementById('game-state').textContent==='Bereit zum Start'`);
  await route('saves');await click('backup-button');
  await until(()=>evaluate(`document.getElementById('save-backups').textContent === '2'`),'Backup status not updated');
  await until(()=>evaluate(`!document.getElementById('notice').hidden`),'Backup feedback missing');
  await screenshot('notification-desktop.png');
  await until(()=>evaluate(`document.getElementById('notice').hidden`),'Success notice did not automatically disappear',8000);
  results.push('Real success notification automatically disappears after six seconds');

  assert.equal(posts.at(-1).path,'/api/saves/backup');results.push('Backup updates only from backend result');
  status.saves.can_backup=false;await refresh(`document.getElementById('backup-button').disabled`);await check('Unavailable backup remains disabled',`document.getElementById('backup-button').disabled`);
  status.saves.can_backup=true;await refresh(`!document.getElementById('backup-button').disabled`);
  const beforeMods=posts.length;await route('mods');
  await until(()=>evaluate(`!document.getElementById('mods-inventory').hidden && !document.getElementById('mods-open').disabled`),'Community inventory missing');
  await check('Empty inventory offers the actual folder and provider installation guidance',`!document.getElementById('mods-empty').hidden && document.getElementById('mods-empty').textContent.includes('Noch keine Add-ons') && document.getElementById('mods-folder').textContent==='/synthetic/Community' && !document.getElementById('mods-empty-help').hidden`);
  assert.equal(posts.length,beforeMods);results.push('Opening the Mods tab only reads inventory');
  await evaluate(`document.getElementById('notice').hidden=true`);await screenshot('mods-empty-desktop.png');
  const modRows=[{id:'synthetic-aircraft',name:'Example Aircraft',version:'1.2.3',creator:'Example Developer',status:'available',is_link:false},{id:'synthetic-long',name:'<img src=x onerror="window.modInjected=1">'+ 'N'.repeat(400),version:'<svg onload="window.modInjected=1">'+ 'V'.repeat(200),creator:'C'.repeat(300),status:'invalid_manifest',is_link:false},{id:'synthetic-missing',name:'',version:'',creator:'',status:'missing_manifest',is_link:true}];
  mods={...mods,mods:modRows,count:3,scanned_count:3};await click('mods-refresh');
  await until(()=>evaluate(`document.querySelectorAll('#mods-list .mod-row').length===3`),'Manifest rows missing');
  await check('Mod names, versions and folders render as literal bounded text',`document.querySelector('#mods-list h3').textContent==='Example Aircraft' && document.querySelector('#mods-list dd').textContent==='1.2.3' && document.querySelectorAll('#mods-list h3')[1].textContent.length===240 && document.querySelectorAll('#mods-list dd')[1].textContent.length===100 && !window.modInjected && !document.querySelector('#mods-list img,#mods-list svg')`);
  await check('Invalid or absent manifests are explicit, not ready/compatible claims',`document.getElementById('mods-list').textContent.includes('Manifest ungültig') && document.getElementById('mods-list').textContent.includes('manifest.json fehlt') && document.querySelectorAll('#mods-list dd')[2].textContent==='Nicht angegeben' && document.getElementById('mods-empty').hidden`);
  await screenshot('mods-desktop.png');
  status.runtime.path='/synthetic/another-runtime';await refresh(`document.getElementById('mods-open').disabled && !document.getElementById('mods-stale').hidden`);
  await check('Switching runtime cannot open a folder from a stale inventory',`document.getElementById('mods-open').disabled && document.querySelectorAll('#mods-list .mod-row').length===3`);
  mods.folder_path='/synthetic/another-Community';await click('mods-refresh');await until(()=>evaluate(`!document.getElementById('mods-open').disabled && document.getElementById('mods-folder').textContent==='/synthetic/another-Community'`),'Changed runtime inventory did not refresh');
  status.game={state:'running',managed:true,can_start:false,can_stop:true};await refresh(`document.getElementById('game-state').textContent==='Simulator läuft'`);
  await check('Community folder may open while the simulator is running',`!document.getElementById('mods-open').disabled`);
  await click('mods-open');await until(()=>posts.length===beforeMods+1,'Folder-open request missing');
  assert.deepEqual(posts.at(-1).body,{});assert.equal(posts.at(-1).path,'/api/mods/open-folder');assert.equal(posts.at(-1).token,csrf);results.push('Folder opening sends no client path and uses the CSRF token');
  status.game={state:'stopped',managed:false,can_start:true,can_stop:false};await refresh(`document.getElementById('game-state').textContent==='Bereit zum Start'`);
  modsUnavailable=true;await click('mods-refresh');await until(()=>evaluate(`!document.getElementById('mods-error').hidden`),'Inventory read failure missing');
  await check('Failed refresh preserves a labelled stale list and disables folder opening',`!document.getElementById('mods-stale').hidden && document.querySelectorAll('#mods-list .mod-row').length===3 && document.getElementById('mods-open').disabled && document.getElementById('mods-empty').hidden`);
  modsUnavailable=false;
  for(const [state,phrase] of [['unconfigured','Spielinstallation'],['unknown','verwendeten Community-Ordner'],['missing','fehlt'],['ambiguous','mehrere'],['error','nicht lesbar']]){
    mods={...mods,state,mods:[],count:0,can_open:false};await click('mods-refresh');
    await until(()=>evaluate(`document.getElementById('mods-message').textContent.includes(${JSON.stringify(phrase)})`),'Missing unavailable reason '+state);
    await check('Unavailable Community state never claims empty success: '+state,`document.getElementById('mods-inventory').hidden && document.getElementById('mods-open').disabled`);
  }
  mods={...mods,state:'ready',mods:[],count:20,can_open:true,limited:true};await click('mods-refresh');
  await until(()=>evaluate(`!document.getElementById('mods-limited').hidden`),'Limited inventory banner missing');
  await check('Partial inventory never claims the full Community folder is empty',`document.getElementById('mods-empty').textContent.includes('geprüften Teil') && document.getElementById('mods-empty-help').hidden`);
  mods={...mods,mods:modRows,count:3,can_open:true,limited:false};await click('mods-refresh');await until(()=>evaluate(`document.querySelectorAll('#mods-list .mod-row').length===3`),'Inventory recovery failed');
  await language('en');await until(()=>evaluate(`document.getElementById('mods-open-label').textContent==='Open Community folder'`),'Mods English labels missing');
  await check('Mods language switch preserves raw add-on names and versions',`document.querySelector('#mods-list h3').textContent==='Example Aircraft' && document.querySelector('#mods-list dd').textContent==='1.2.3' && document.getElementById('mods-list').textContent.includes('Invalid manifest')`);
  await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
  await check('Six navigation items and long mod data fit the mobile viewport',`document.documentElement.scrollWidth<=innerWidth && document.querySelectorAll('[data-nav]').length===6 && [...document.querySelectorAll('[data-nav]')].every(x=>x.getBoundingClientRect().width>0)`);await screenshot('mods-en-mobile.png');
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});await language('de');
  // Game updates use only synthetic metadata and the shared job lifecycle.
  setup.job=null;setup.state='idle';status.game={state:'stopped',managed:false,can_start:true,can_stop:false};
  const beforeUpdates=posts.length;await route('updates');
  await until(()=>evaluate(`!document.getElementById('update-check').disabled`),'Update check not available');
  await check('Updates show the actual installed version and unknown remote version',`document.getElementById('update-installed').textContent==='1.8.16.0' && document.getElementById('update-latest').textContent==='Noch nicht geprüft' && document.getElementById('update-title').textContent==='Noch nicht nach Updates gesucht'`);
  assert.equal(posts.length,beforeUpdates);results.push('Entering Updates never checks remotely or signs in automatically');
  await screenshot('updates-idle-desktop.png');
  updateDelay=200;const beforePolls=updateReplies;
  await evaluate(`(()=>{const b=document.getElementById('update-check');b.focus();window.updateButton=b;window.updateButtonChild=b.firstChild;window.updatePollFlicker=false;window.updatePollObserver=new MutationObserver(()=>{if(b.disabled||b.hidden||document.activeElement!==b||b!==document.getElementById('update-check')||b.firstChild!==window.updateButtonChild)window.updatePollFlicker=true;});window.updatePollObserver.observe(document.getElementById('view-updates'),{subtree:true,childList:true,attributes:true,attributeFilter:['disabled','hidden']});})()`);
  await until(()=>updateReplies>=beforePolls+3,'Three background update polls did not finish');
  await check('Unchanged background polls preserve update button DOM, focus and enabled state',`!window.updatePollFlicker && document.activeElement===window.updateButton && !window.updateButton.disabled`);
  await evaluate(`window.updatePollObserver.disconnect()`);updateDelay=0;
  updateWaiting=false;updateBarrier=new Promise(resolve=>{releaseUpdate=resolve;});
  await until(()=>updateWaiting,'Failure poll did not reach barrier');
  await click('update-check');updateUnavailable=true;
  releaseUpdate();updateBarrier=null;releaseUpdate=null;
  await until(()=>evaluate(`!document.getElementById('update-error').hidden && !document.getElementById('update-refresh').disabled`),'Failed poll did not finish reconciliation');
  assert.equal(posts.length,beforeUpdates);results.push('Queued check does not post after its pending status read fails');
  await check('Failed poll keeps stale update actions locked',`document.getElementById('update-check').disabled && !document.getElementById('update-stale').hidden`);
  updateUnavailable=false;await click('update-refresh');
  await until(()=>evaluate(`!document.getElementById('update-check').disabled`),'Update status did not recover');
  statusWaiting=false;statusBarrier=new Promise(resolve=>{releaseStatus=resolve;});
  updateWaiting=false;updateBarrier=new Promise(resolve=>{releaseUpdate=resolve;});
  await until(()=>updateWaiting,'Background update poll did not reach barrier');
  await click('update-check');
  await check('Explicit check during a background poll immediately reserves controls',`document.getElementById('update-check').disabled && document.getElementById('launch-button').disabled`);
  assert.equal(posts.length,beforeUpdates);results.push('Explicit check waits for the prior read before posting');
  releaseUpdate();updateBarrier=null;releaseUpdate=null;
  await until(async()=>statusWaiting && await evaluate(`document.getElementById('update-latest').textContent==='1.9.0.0'`),'Pending update reconciliation missing');
  await route('installation');await until(()=>evaluate(`!document.getElementById('runtime-form').hidden`),'Ready update did not restore setup');
  await check('Pending update reconciliation also locks setup, before ready state releases it',`document.getElementById('config-button').disabled && document.getElementById('launch-button').disabled`);
  releaseStatus();statusBarrier=null;releaseStatus=null;await route('updates');
  await until(()=>evaluate(`!document.getElementById('update-start').disabled && !document.getElementById('launch-button').disabled`),'Ready update did not release launcher');
  assert.equal(posts.at(-1).path,'/api/game-update/check');assert.deepEqual(posts.at(-1).body,{});results.push('Manual update check uses no implicit sign-in and never starts a download');
  await check('Ready check displays both actual versions and leaves play available',`document.getElementById('update-latest').textContent==='1.9.0.0' && document.getElementById('update-title').textContent==='Eine neue Spielversion ist verfügbar' && !document.getElementById('launch-button').disabled`);
  await screenshot('updates-ready-desktop.png');
  status.game={state:'running',managed:true,can_start:false,can_stop:true};await refresh(`document.getElementById('update-start').disabled`);
  await check('Running game blocks ready update without discarding checked versions',`!document.getElementById('update-busy').hidden && document.getElementById('update-latest').textContent==='1.9.0.0' && document.getElementById('update-start').disabled`);
  status.game={state:'stopped',managed:false,can_start:true,can_stop:false};await refresh(`!document.getElementById('update-start').disabled`);
  await click('update-start');await until(()=>evaluate(`!document.getElementById('update-pause').disabled && document.getElementById('launch-button').disabled`),'Update download missing');
  assert.equal(posts.at(-1).path,'/api/game-update/start');assert.deepEqual(posts.at(-1).body,{check_id:'update-fixture'});assert.equal(posts.at(-1).token,csrf);results.push('Update start sends the exact checked revision job and CSRF');
  await check('Real download phase shows indeterminate progress rather than invented percent',`!document.getElementById('update-progress').hidden && !document.getElementById('update-progress').hasAttribute('value') && document.getElementById('update-title').textContent==='Update wird heruntergeladen'`);
  setup.job={...setup.job,transfer:{...transferFixture,kind:'game',received_bytes:2_000_000_000}};
  await click('update-refresh');
  await eventualCheck('Received but unverified update bytes cannot claim 100 percent',`document.getElementById('update-progress').value===99.9 && document.getElementById('update-transfer').textContent==='2 GB von 2 GB · 99,9 %'`);
  setup.job={...setup.job,transfer:{...setup.job.transfer,verified_bytes:2_000_000_000}};
  await click('update-refresh');
  await eventualCheck('Uncommitted zero-byte files prevent a premature 100 percent',`document.getElementById('update-progress').value===99.9`);
  setup.job={...setup.job,transfer:{...setup.job.transfer,completed_files:3}};
  await click('update-refresh');
  await eventualCheck('Fully verified bytes may show 100 while installation remains active',`document.getElementById('update-progress').value===100 && document.getElementById('update-title').textContent==='Update wird heruntergeladen' && document.getElementById('launch-button').disabled`);
  await route('installation');await until(()=>evaluate(`!document.getElementById('setup-update-link').hidden`),'Shared update link missing');
  await check('Setup cannot display or start an update as a new installation',`document.getElementById('setup-job').hidden && document.getElementById('runtime-form').hidden && document.getElementById('setup-steps').hidden && document.getElementById('setup-form-detail').hidden`);
  await route('updates');const beforeUpdatePause=posts.length;await click('update-pause');
  await until(()=>evaluate(`document.getElementById('update-title').textContent==='Download wird pausiert'`),'Update pausing missing');
  await check('Update pausing cannot be repeated and still permits cancellation',`document.getElementById('update-pause').disabled && document.getElementById('update-resume').hidden && !document.getElementById('update-cancel').disabled`);
  setup.job={...setup.job,phase:'paused',can_resume:true};await click('update-refresh');
  await until(()=>evaluate(`!document.getElementById('update-resume').disabled`),'Update pause not confirmed');
  await language('en');await until(()=>evaluate(`document.getElementById('update-title').textContent==='Download paused'`),'Update language switch failed');
  await check('Paused update keeps verified byte progress and session resume controls',`document.getElementById('update-progress').value===100 && document.getElementById('update-transfer').textContent==='Paused · 2 GB of 2 GB · 100 %' && document.getElementById('update-help').textContent.includes('Keep Flightdeck open') && document.getElementById('update-resume').textContent==='Resume download'`);
  await screenshot('updates-paused-en-desktop.png');
  await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
  await check('Update actions and six sections fit the mobile width',`document.documentElement.scrollWidth<=innerWidth && [...document.querySelectorAll('#update-actions button:not([hidden]),.update-actions button:not([hidden])')].every(b=>b.getBoundingClientRect().right<=innerWidth)`);await screenshot('updates-paused-en-mobile.png');
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});await call('Page.reload');
  await until(()=>evaluate(`document.getElementById('update-title')?.textContent==='Download paused' && !document.getElementById('update-resume').disabled`),'Paused update did not survive reload');
  assert.deepEqual(posts.slice(beforeUpdatePause).map(p=>p.path),['/api/setup/pause']);results.push('Language switch and reload preserve paused update without auto-resuming');
  await click('update-resume');await until(()=>evaluate(`!document.getElementById('update-pause').disabled`),'Update resume missing');
  assert.equal(posts.at(-1).path,'/api/setup/resume');assert.equal(posts.at(-1).body.job_id,'update-fixture');results.push('Update resume reuses the existing shared job');
  await click('update-cancel');await until(()=>evaluate(`!document.getElementById('update-check').disabled && !document.getElementById('launch-button').disabled`),'Cancelled update did not release launch');
  assert.equal(posts.at(-1).path,'/api/setup/cancel');assert.equal(posts.at(-1).body.job_id,'update-fixture');results.push('Update cancellation uses the shared job and releases launch');
  await route('installation');await until(()=>evaluate(`!document.getElementById('runtime-form').hidden`),'Completed update still blocks setup');
  await check('Terminal update does not strand setup behind a hidden reset button',`document.getElementById('setup-update-link').hidden && !document.getElementById('setup-steps').hidden`);
  await language('de');await route('updates');
  setup.job=null;gameUpdate={...gameUpdate,latest_version:null,update_available:null,can_start:false,auth_required:true};await click('update-refresh');
  await until(()=>evaluate(`!document.getElementById('update-sign-in').disabled`),'Explicit sign-in missing');
  const beforeSignIn=posts.length;await check('Missing authentication offers only an explicit sign-in action',`document.getElementById('update-check').hidden && document.getElementById('update-title').textContent==='Anmeldung zum Prüfen erforderlich'`);
  await click('update-sign-in');await until(()=>posts.length===beforeSignIn+1&&setup.job?.state==='ready','Explicit sign-in request missing');assert.deepEqual(posts.at(-1).body,{sign_in:true});results.push('Only the sign-in button opts into interactive Microsoft authentication');
  await until(()=>evaluate(`!document.getElementById('update-cancel').disabled`),'Ready cancellation missing');await click('update-cancel');await until(()=>evaluate(`!document.getElementById('update-check').disabled`),'Ready update not dismissed');
  updateUnavailable=true;await click('update-refresh');await until(()=>evaluate(`!document.getElementById('update-error').hidden`),'Update failure missing');
  await check('Failed status read preserves labelled stale versions and disables mutations',`!document.getElementById('update-stale').hidden && document.getElementById('update-check').disabled && document.getElementById('update-latest').textContent==='1.9.0.0'`);
  updateUnavailable=false;setup.job=null;gameUpdate={...gameUpdate,available:false,unavailable_reason:'Synthetic CLI cannot safely update.',installed_version:null,latest_version:null,update_available:null,can_rollback:true};await click('update-refresh');
  await until(()=>evaluate(`!document.getElementById('update-rollback').disabled`),'Rollback should not need current CLI capability');
  await check('Missing update capability never invents current versions or blocks a valid rollback',`document.getElementById('update-installed').textContent==='Nicht bekannt' && document.getElementById('update-latest').textContent==='Noch nicht geprüft' && document.getElementById('update-check').disabled && document.getElementById('update-message').textContent==='Synthetic CLI cannot safely update.'`);
  const beforeRollback=posts.length;await click('update-rollback');await check('Rollback requires a second explicit confirmation',`!document.getElementById('update-rollback-confirm').hidden`);assert.equal(posts.length,beforeRollback);
  await click('update-rollback-no');assert.equal(posts.length,beforeRollback);await click('update-rollback');await click('update-rollback-yes');await until(()=>posts.length===beforeRollback+1,'Rollback mutation missing');assert.equal(posts.at(-1).path,'/api/game-update/rollback');assert.deepEqual(posts.at(-1).body,{});results.push('Confirmed rollback sends no caller-supplied file paths');
  gameUpdate={...gameUpdate,available:true,installed_version:'1.9.0.0',latest_version:'1.9.0.0',update_available:false,can_rollback:false};await click('update-refresh');
  await until(()=>evaluate(`document.getElementById('update-title').textContent==='Deine Spielversion ist aktuell'`),'Server-confirmed current status missing');
  await eventualCheck('Only a completed server comparison can report up-to-date',`document.getElementById('update-start').hidden && !document.getElementById('update-check').disabled`);
  failNext=true;await click('update-check');await until(()=>evaluate(`document.getElementById('update-error').textContent.includes('<img')`),'Update action error missing');
  await check('Update failure is literal text and remains visible after reconciliation',`!window.injected && !document.querySelector('#update-error img') && document.getElementById('update-error').textContent.includes('Backend-Fehler')`);
  await click('update-refresh');await until(()=>evaluate(`document.getElementById('update-error').hidden`),'Explicit refresh did not clear action error');
  await until(()=>evaluate(`!document.getElementById('update-verify').disabled`),'Integrity controls did not finish status reconciliation');
  const beforeIntegrity=posts.length;
  await eventualCheck('File verification is an explicit separate local action',`!document.getElementById('update-verify').disabled && document.getElementById('integrity-results').hidden && document.getElementById('integrity-status').textContent==='Noch keine Dateiprüfung durchgeführt.'`);
  await click('update-verify');await until(()=>evaluate(`document.getElementById('update-title').textContent==='Spieldateien werden geprüft' && !document.getElementById('update-cancel').disabled`),'Local verification did not start');
  assert.equal(posts.at(-1).path,'/api/game-update/verify');assert.deepEqual(posts.at(-1).body,{});results.push('Verify only requests the local integrity endpoint, no repair/download');
  await check('File verification reserves launch and is cancellable without fake progress',`document.getElementById('launch-button').disabled && document.getElementById('update-repair-check').disabled && !document.getElementById('update-progress').hasAttribute('value')`);
  await click('update-cancel');await until(()=>evaluate(`!document.getElementById('update-verify').disabled`),'Verification did not cancel');
  gameUpdate.integrity.result={checked:50,missing:1,changed:2,unreadable:0,total:50,healthy:false};
  setup.job={...setup.job,state:'complete',phase:'complete',operation:'verify',message:'Synthetic differences found.'};await click('update-refresh');
  await until(()=>evaluate(`!document.getElementById('integrity-results').hidden`),'Integrity counters missing');
  await check('Integrity reports exact counters, not invented file names or summed totals',`document.getElementById('integrity-checked').textContent==='50' && document.getElementById('integrity-missing').textContent==='1' && document.getElementById('integrity-changed').textContent==='2' && document.getElementById('integrity-unreadable').textContent==='0' && document.getElementById('integrity-total').textContent==='50' && document.getElementById('integrity-status').textContent==='Die Dateiprüfung hat Abweichungen gefunden.'`);
  const repairsBefore=posts.filter(p=>p.path==='/api/game-update/repair/check').length;
  await language('en');await until(()=>evaluate(`document.getElementById('integrity-status').textContent==='The file check found differences.'`),'Integrity translation missing');
  await check('File results translate while full repair scope stays explicit',`document.getElementById('update-verify').textContent==='Check files' && document.getElementById('update-repair-check').textContent==='Prepare repair' && document.getElementById('integrity-repair-help').textContent.includes('complete base game again')`);
  assert.equal(posts.filter(p=>p.path==='/api/game-update/repair/check').length,repairsBefore);results.push('A failed file check or language switch never starts a repair');
  await screenshot('integrity-results-en-desktop.png');
  await click('update-repair-check');await until(()=>evaluate(`!document.getElementById('update-start').disabled && document.getElementById('update-title').textContent==='Repair prepared'`),'Same-version repair not prepared');
  assert.equal(posts.at(-1).path,'/api/game-update/repair/check');assert.deepEqual(posts.at(-1).body,{});results.push('Preparing repair checks the Store but still waits for explicit download start');
  await eventualCheck('Same-version repair may download only after explicit start',`document.getElementById('update-installed').textContent===document.getElementById('update-latest').textContent && document.getElementById('update-start-label').textContent==='Download game again' && !document.getElementById('launch-button').disabled`);
  await click('update-start');await until(()=>evaluate(`!document.getElementById('update-pause').disabled`),'Repair download missing');
  assert.deepEqual(posts.at(-1).body,{check_id:'repair-fixture'});results.push('Repair start is bound to its exact check ID and shares pause/cancel');
  setup.job={...setup.job,transfer:{...transferFixture,kind:'game'}};await click('update-refresh');
  await eventualCheck('Repair uses the shared English byte progress presentation',`document.getElementById('update-transfer').textContent==='512 MB of 2 GB · 25.6 %' && document.getElementById('update-progress').value===25.6`);
  setup.job={...setup.job,transfer:{...setup.job.transfer,received_bytes:'512000000'}};await click('update-refresh');
  await eventualCheck('Malformed transfer is discarded without NaN or a fabricated percentage',`document.getElementById('update-transfer').hidden && !document.getElementById('update-progress').hasAttribute('value')`);
  await click('update-cancel');await until(()=>evaluate(`!document.getElementById('update-repair-check').disabled`),'Repair cancellation missing');
  setup.job={...setup.job,state:'failed',phase:'failed',operation:'repair',error:'Synthetic authentication needed.'};gameUpdate.auth_required=true;gameUpdate.can_check=false;await click('update-refresh');
  await until(()=>evaluate(`!document.getElementById('update-sign-in').disabled`),'Repair sign-in action missing');
  await click('update-sign-in');await until(()=>evaluate(`!document.getElementById('update-start').disabled`),'Repair sign-in did not restore ready plan');
  assert.equal(posts.at(-1).path,'/api/game-update/repair/check');assert.deepEqual(posts.at(-1).body,{sign_in:true});results.push('Repair authentication stays on repair endpoint and requires explicit sign-in');
  await click('update-cancel');await until(()=>evaluate(`!document.getElementById('update-repair-check').disabled`),'Repair plan not dismissed');
  setup.job=null;gameUpdate.can_check=true;gameUpdate.integrity={available:false,can_check:false,unavailable_reason:'Synthetic legacy installation has no complete index.',result:null};await click('update-refresh');
  await until(()=>evaluate(`document.getElementById('update-verify').disabled && !document.getElementById('update-repair-check').disabled`),'Legacy integrity capability mismatch');
  await eventualCheck('Legacy missing index never claims healthy while genuine full repair remains available',`document.getElementById('integrity-results').hidden && document.getElementById('integrity-status').textContent==='Synthetic legacy installation has no complete index.'`);
  status.game={state:'running',managed:true,can_start:false,can_stop:true};await refresh(`document.getElementById('update-repair-check').disabled`);await check('Active game blocks both verification and repair',`document.getElementById('update-verify').disabled && document.getElementById('update-repair-check').disabled`);
  status.game={state:'stopped',managed:false,can_start:true,can_stop:false};await refresh(`!document.getElementById('update-repair-check').disabled`);
  gameUpdate.integrity={available:true,can_check:true,result:{checked:50,missing:0,changed:0,unreadable:0,total:50,healthy:true}};await click('update-refresh');
  await until(()=>evaluate(`document.getElementById('integrity-status').textContent==='No differences found.'`),'Verified healthy result missing');
  await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await evaluate(`document.getElementById('integrity-title').scrollIntoView({block:'start'})`);
  await check('Integrity counters and repair actions fit on mobile',`document.documentElement.scrollWidth<=innerWidth && document.getElementById('integrity-total').textContent==='50'`);await screenshot('integrity-en-mobile.png');
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});await evaluate(`scrollTo(0,0)`);await language('de');
  await route('saves');await until(()=>evaluate(`!document.getElementById('cloud-check').disabled`),'Cloud check unavailable');
  await check('Automatic saves are primary and manual tools start collapsed',`!document.getElementById('cloud-advanced').open && !document.getElementById('cloud-check').checkVisibility() && document.getElementById('auto-cloud-title').getClientRects().length>0`);
  await evaluate(`document.getElementById('cloud-advanced').open=true`);
  await check('Cloud starts unknown, without an invented empty inventory',`document.getElementById('cloud-results').hidden && document.getElementById('cloud-status').textContent==='Noch nicht geprüft' && document.getElementById('cloud-title').textContent==='Xbox-Cloud-Spielstände'`);
  await evaluate(`window.cloudButton=document.getElementById('cloud-check');cloudButton.focus();window.cloudDisabled=[];window.cloudObserver=new MutationObserver(()=>cloudDisabled.push(cloudButton.disabled));cloudObserver.observe(cloudButton,{attributes:true,attributeFilter:['disabled']})`);
  const cloudReads=cloudReplies;await until(()=>cloudReplies>=cloudReads+2,'Cloud background polling missing');
  await check('Quiet cloud polls preserve button node, focus and enabled state',`document.getElementById('cloud-check')===cloudButton && document.activeElement===cloudButton && cloudDisabled.length===0`);
  await evaluate(`cloudObserver.disconnect()`);
  cloudBarrier=new Promise(resolve=>releaseCloud=resolve);await until(()=>cloudWaiting,'Cloud barrier did not receive a poll');
  const beforeCloudPosts=posts.length;await click('cloud-check');
  await check('Cloud click during quiet poll immediately reserves other actions',`document.getElementById('backup-button').disabled && document.getElementById('launch-button').disabled`);
  assert.equal(posts.length,beforeCloudPosts);releaseCloud();cloudBarrier=null;cloudWaiting=false;
  await until(()=>posts.some(p=>p.path==='/api/cloud-saves/check'),'Cloud check intent lost during poll');
  await until(()=>evaluate(`!document.getElementById('cloud-cancel').hidden`),'Running cloud job missing');
  await route('overview');await check('Running cloud job keeps launch reserved away from Saves',`document.getElementById('launch-button').disabled`);
  cloudData={...cloudData,can_check:true,can_download:true,can_cancel:false,job:{...cloudData.job,state:'succeeded',finished_at:'2026-09-18T12:00:00Z',message:'Synthetic cloud inventory checked.',result:{container_count:2,blob_count:null,total_bytes:1024,downloaded:false,rechecked:false}}};
  await until(()=>evaluate(`!document.getElementById('launch-button').disabled`),'Completed cloud job left reservation stuck');results.push('Active cloud job polling releases reservation on completion outside Saves');
  await route('saves');await until(()=>evaluate(`!document.getElementById('cloud-results').hidden`),'Cloud counts missing');
  await check('Checked cloud inventory shows actual counts and unknown blob count',`document.getElementById('cloud-containers').textContent==='2' && document.getElementById('cloud-blobs').textContent==='Nicht ermittelt'`);
  await click('cloud-download');await until(()=>cloudData.job?.operation==='download','Cloud download not submitted');await until(()=>evaluate(`!document.getElementById('cloud-cancel').disabled`),'Cloud cancellation unavailable');
  await click('cloud-cancel');await until(()=>evaluate(`document.getElementById('cloud-status').textContent==='Cloud-Anfrage abgebrochen'`),'Cloud cancellation missing');
  assert.deepEqual(posts.filter(p=>p.path==='/api/cloud-saves/cancel').at(-1).body,{job_id:'cloud-download'});results.push('Cloud cancellation posts only the exact running job ID');
  cloudData.job={id:'completed-copy',operation:'download',state:'succeeded',finished_at:'2026-09-18T12:30:00Z',message:'<img src=x onerror="window.injected=1">',result:{container_count:2,blob_count:3,total_bytes:1024,downloaded:true,rechecked:true,snapshot_id:'synthetic-copy',consistency:'rechecked-unlocked'}};
  await click('cloud-refresh');await until(()=>evaluate(`!document.getElementById('cloud-copy-note').hidden`),'Downloaded copy result missing');
  await check('Cloud result strings stay text; copy is never called an active save or sync',`!window.injected && !document.querySelector('#cloud-message img') && document.getElementById('cloud-blobs').textContent==='3' && document.getElementById('cloud-copy-note').textContent.includes('nicht als aktiver')`);
  cloudData.job.message='';await click('cloud-refresh');await until(()=>evaluate(`!document.getElementById('cloud-message').textContent.includes('<img')`),'Clean cloud preview missing');
  await evaluate(`document.getElementById('cloud-title').scrollIntoView({block:'start'})`);await screenshot('cloud-saves-de-desktop.png');await language('en');await until(()=>evaluate(`document.getElementById('cloud-status').textContent==='Cloud copy saved'`),'Cloud English title missing');
  await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await evaluate(`document.getElementById('cloud-title').scrollIntoView({block:'start'})`);
  await check('English mobile cloud actions and import explanation fit',`document.documentElement.scrollWidth<=innerWidth && document.getElementById('cloud-title').textContent==='Xbox cloud saves' && document.querySelector('.cloud-limit').textContent.includes('backs up')`);await screenshot('cloud-saves-en-mobile.png');
  await click('cloud-prepare-import');await until(()=>cloudData.job?.operation==='prepare-import','Comparison request missing');
  const cloudPreview={id:'preview-fixture',container_count:2,blob_count:3,total_bytes:1024,local_exists:true,local_container_count:2,add_count:1,replace_count:1,delete_count:1,unchanged_count:0,conflict_count:2};
  const cloudCompared={container_count:2,blob_count:3,total_bytes:1024,downloaded:true,rechecked:true,prepared_for_import:true};
  cloudData={...cloudData,can_check:true,can_download:true,can_import:true,can_cancel:false,plan:cloudPreview,job:{...cloudData.job,state:'succeeded',message:'',result:cloudCompared}};
  await click('cloud-refresh');await until(()=>evaluate(`!document.getElementById('cloud-import').disabled`),'Import preview missing');
  await check('Comparison exposes conflicts and backup notice without importing',`!document.getElementById('cloud-plan').hidden && document.getElementById('cloud-plan-conflicts').textContent.includes('2 containers') && document.getElementById('cloud-plan').textContent.includes('backup') && document.documentElement.scrollWidth<=innerWidth`);
  assert.equal(posts.filter(p=>p.path==='/api/cloud-saves/import').length,0);results.push('Preparing an import never applies it automatically');
  await evaluate(`document.getElementById('cloud-plan').scrollIntoView({block:'center'})`);await screenshot('cloud-comparison-en-mobile.png');
  await click('cloud-discard-plan');await until(()=>evaluate(`document.getElementById('cloud-plan').hidden`),'Keep-local action failed');
  assert.equal(posts.filter(p=>p.path==='/api/cloud-saves/import').length,0);results.push('Keeping local saves dismisses the exact comparison without importing');
  cloudData={...cloudData,can_import:true,plan:cloudPreview};await click('cloud-refresh');await until(()=>evaluate(`!document.getElementById('cloud-import').disabled`),'Second preview missing');
  await click('cloud-import');await until(()=>cloudData.job?.operation==='import','Import not requested');await until(()=>evaluate(`!document.getElementById('cloud-cancel').disabled`),'Running import not shown');
  await check('An active import reserves launch and other cloud actions',`document.getElementById('launch-button').disabled && document.getElementById('cloud-prepare-import').disabled && document.getElementById('cloud-import').disabled`);
  cloudData={...cloudData,can_check:true,can_download:true,can_cancel:false,job:{...cloudData.job,state:'succeeded',message:'',result:{...cloudCompared,imported:true,durability_confirmed:true,backup_id:'backup-fixture'}}};
  await click('cloud-refresh');await until(()=>evaluate(`document.getElementById('cloud-status').textContent==='Cloud saves imported'`),'Imported state missing');
  await check('Successful import is distinct from an unused downloaded copy',`document.getElementById('cloud-copy-note').hidden && document.getElementById('cloud-plan').hidden`);
  cloudData={...cloudData,can_import:true,plan:{...cloudPreview,id:'empty-preview',container_count:0,blob_count:0,total_bytes:0,add_count:0,replace_count:0,delete_count:2,conflict_count:2}};
  await click('cloud-refresh');await until(()=>evaluate(`!document.getElementById('cloud-empty-warning').hidden`),'Empty cloud warning missing');
  await check('Empty cloud makes local removal explicit',`document.getElementById('cloud-empty-warning').textContent.includes('removes all active local saves')`);
  await language('de');await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});
  await evaluate(`document.getElementById('cloud-plan').scrollIntoView({block:'center'})`);await screenshot('cloud-comparison-de-desktop.png');
  await check('German comparison labels are translated',`document.getElementById('cloud-import').textContent==='Cloud übernehmen' && document.getElementById('cloud-plan-summary').textContent==='0 Container in der Cloud · 2 lokal'`);
  await click('cloud-discard-plan');await until(()=>evaluate(`document.getElementById('cloud-plan').hidden`),'Empty preview discard failed');
  await language('en');
  cloudData={...cloudData,mode:'manual_sync',sync_supported:true,can_upload:true,plan:cloudPreview,restore_id:'backup-fixture',can_restore:true};
  await click('cloud-refresh');await until(()=>evaluate(`!document.getElementById('cloud-upload').disabled`),'Upload unavailable');
  await check('Upload choice states cloud additions, replacements and removals',`!document.getElementById('cloud-upload-choice').hidden && document.getElementById('cloud-upload-changes').textContent.includes('replace 1') && document.getElementById('cloud-mode').textContent==='Sync manually'`);
  await click('cloud-restore');await until(()=>cloudData.job?.operation==='restore','Restore missing');
  cloudData={...cloudData,can_cancel:false,job:{...cloudData.job,state:'succeeded',message:'',result:{container_count:2,blob_count:3,total_bytes:1024,imported:true,restored:true,durability_confirmed:true,downloaded:false,rechecked:false}}};
  await click('cloud-refresh');await until(()=>evaluate(`document.getElementById('cloud-status').textContent==='Local saves restored'`),'Restore success missing');
  await check('Restore uses the exact server backup and consumes its action',`document.getElementById('cloud-restore').hidden && document.getElementById('cloud-plan').hidden`);
  cloudData={...cloudData,can_upload:true,plan:cloudPreview};await click('cloud-refresh');await until(()=>evaluate(`!document.getElementById('cloud-upload').disabled`),'Upload recompare missing');
  await click('cloud-upload');await until(()=>cloudData.job?.operation==='upload','Upload choice missing');await until(()=>evaluate(`!document.getElementById('cloud-cancel').disabled`),'Upload progress missing');
  await check('Upload reserves the launcher and other save mutations',`document.getElementById('launch-button').disabled && document.getElementById('cloud-import').disabled && document.getElementById('backup-button').disabled`);
  cloudData={...cloudData,can_cancel:false,job:{...cloudData.job,state:'failed',message:'Some cloud saves may have changed. Compare again.',recovery_required:true,result:null}};
  await click('cloud-refresh');await until(()=>evaluate(`document.getElementById('cloud-status').textContent==='Upload not fully confirmed'`),'Partial upload warning missing');
  await check('Partial upload is never presented as unchanged or successfully synced',`document.getElementById('cloud-results').hidden && document.getElementById('cloud-message').textContent.includes('may have changed')`);
  cloudData={...cloudData,can_upload:true,plan:{...cloudPreview,local_container_count:0,delete_count:0,replace_count:0,add_count:2}};
  await click('cloud-refresh');await until(()=>evaluate(`!document.getElementById('cloud-local-empty-warning').hidden`),'Empty local upload warning missing');
  await check('Uploading empty local saves explicitly warns about cloud removal',`document.getElementById('cloud-local-empty-warning').textContent.includes('removes all cloud saves')`);
  cloudData={...cloudData,plan:null,can_upload:false,job:{id:'verified-upload',operation:'upload',state:'succeeded',message:'',result:{container_count:2,blob_count:3,total_bytes:1024,downloaded:false,rechecked:true,uploaded:true,changed_containers:2,lease_released:true,baseline_saved:true}}};
  await click('cloud-refresh');await until(()=>evaluate(`document.getElementById('cloud-status').textContent==='Xbox cloud updated'`),'Verified upload missing');
  await check('Verified upload shows confirmed counts and no unused-copy label',`document.getElementById('cloud-containers').textContent==='2' && document.getElementById('cloud-copy-note').hidden`);
  await evaluate(`document.getElementById('cloud-title').scrollIntoView({block:'start'})`);await screenshot('cloud-sync-en-desktop.png');
  cloudUnavailable=true;await click('cloud-refresh');await until(()=>evaluate(`!document.getElementById('cloud-error').hidden`),'Cloud API failure missing');
  await check('Missing cloud API is an error, never an empty inventory or enabled action',`document.getElementById('cloud-check').disabled && document.getElementById('cloud-download').disabled && !document.getElementById('cloud-stale').hidden && document.getElementById('cloud-containers').textContent==='2'`);
  cloudUnavailable=false;await click('cloud-refresh');await until(()=>evaluate(`!document.getElementById('cloud-check').disabled`),'Cloud recovery failed');
  status.game={...status.game,state:'running'};await refresh(`document.getElementById('cloud-check').disabled`);await check('Running game blocks cloud check and copy',`document.getElementById('cloud-download').disabled && !document.getElementById('cloud-busy').hidden`);
  status.game={state:'stopped',managed:false,can_start:true,can_stop:false};await refresh(`!document.getElementById('cloud-check').disabled`);
  cloudBarrier=new Promise(resolve=>releaseCloud=resolve);await until(()=>cloudWaiting,'Second cloud poll barrier missing');await click('cloud-check');
  const cloudPostCount=posts.filter(p=>p.path==='/api/cloud-saves/check').length;
  status.runtime.path='/synthetic/other-runtime';await refresh(`document.getElementById('current-path').textContent==='/synthetic/other-runtime'`);
  releaseCloud();cloudBarrier=null;cloudWaiting=false;await until(()=>evaluate(`!document.getElementById('cloud-refresh').disabled`),'Runtime-switch request did not settle');
  assert.equal(posts.filter(p=>p.path==='/api/cloud-saves/check').length,cloudPostCount);results.push('Runtime switch during a pending cloud click prevents posting stale intent');
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});await language('de');
  await route('diagnostics');await click('diagnostic-refresh');
  await until(()=>evaluate(`!document.getElementById('diagnostic-download').disabled`),'Diagnostics unavailable');
  await check('Diagnostic export excludes status/CSRF/extra fields',`!document.getElementById('diagnostic-json').textContent.includes(${JSON.stringify(csrf)}) && !document.getElementById('diagnostic-json').textContent.includes('MUST-NOT-EXPORT')`);
  await check('Store failures appear as method and HRESULT without product or account details',`document.getElementById('diagnostic-summary').textContent.includes('Store-API-Aufrufe') && document.getElementById('diagnostic-summary').textContent.includes('XStoreShowPurchaseUIAsync') && document.getElementById('diagnostic-summary').textContent.includes('80004001')`);
  await check('Graphics and sync diagnostics expose the GPU and numeric failure',`document.getElementById('diagnostic-summary').textContent.includes('Grafik und Vulkan') && document.getElementById('diagnostic-summary').textContent.includes('NVIDIA GeForce RTX 4060') && document.getElementById('diagnostic-summary').textContent.includes('Xbox-Cloud-Abgleich') && document.getElementById('diagnostic-summary').textContent.includes('503')`);
  await call('Browser.setDownloadBehavior',{behavior:'allow',downloadPath:join(temp,'downloads')});await click('diagnostic-download');
  await until(async()=>{try{return(await readdir(join(temp,'downloads'))).includes('flightdeck-diagnose.json');}catch{return false;}},'Download missing');
  const exported=JSON.parse(await readFile(join(temp,'downloads/flightdeck-diagnose.json'),'utf8'));
  assert.deepEqual(Object.keys(exported),['summary','checks','generated_at']);results.push('Actual JSON download is safe report only');
  assert.equal(exported.summary.cloud_sync.error_details.http_status,503);assert.equal(exported.summary.graphics.devices[0].name,'NVIDIA GeForce RTX 4060');
  await screenshot('diagnostics-desktop.png');
  await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await check('GPU and cloud diagnostics fit the mobile viewport',`document.documentElement.scrollWidth<=innerWidth`);await screenshot('diagnostics-mobile.png');
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});
  status.runtime.checks=[{label:'<img src=x onerror="window.injected=1">',detail:'<script>window.injected=1</script>',ok:false}];await refresh(`document.getElementById('overview-checks').textContent.includes('<img')`);
  await check('Server check strings cannot inject DOM',`!window.injected && document.querySelectorAll('#overview-checks img,#overview-checks script').length===0 && document.getElementById('overview-checks').textContent.includes('<img')`);
  await evaluate(`location.hash='saves'`);failNext=true;await click('backup-button');
  await until(()=>evaluate(`document.getElementById('notice').classList.contains('error')`),'Backend error not visible');
  await check('Server error strings cannot inject DOM',`!window.injected && document.querySelectorAll('#notice img').length===0 && document.getElementById('notice').textContent.includes('<img')`);
  apiUnavailable=true;await refresh(`document.getElementById('connection-label').textContent.includes('nicht erreichbar')`);await check('Disconnected disables every mutation',`document.getElementById('launch-button').disabled && document.getElementById('config-button').disabled && document.getElementById('backup-button').disabled && document.getElementById('connection-label').textContent.includes('nicht erreichbar')`);
  apiUnavailable=false;status.runtime.checks=checks;await refresh(`document.getElementById('connection').classList.contains('online')`);
  await check('Polling recovers connection',`document.getElementById('connection').classList.contains('online')`);
  await evaluate(`document.getElementById('notice').hidden=true`);await route('overview');
  await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await sleep(100);
  await check('Mobile has no horizontal overflow',`document.documentElement.scrollWidth <= innerWidth`);
  await check('Mobile navigation shows all six sections',`[...document.querySelectorAll('[data-nav]')].every(x=>x.getBoundingClientRect().width>0)`);
  await screenshot('overview-mobile.png');
  await route('installation');await check('Mobile installation has no overflow',`document.documentElement.scrollWidth <= innerWidth`);
  await route('saves');await check('Mobile saves has no overflow',`document.documentElement.scrollWidth <= innerWidth`);await screenshot('saves-mobile.png');

  await language('en');await route('overview');
  await check('English mobile layout has no overflow',`document.documentElement.scrollWidth<=innerWidth && document.getElementById('page-title').textContent==='Overview'`);
  await screenshot('overview-en-mobile.png');
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});
  await screenshot('overview-en-desktop.png');
  await call('Page.reload');
  await until(()=>evaluate(`document.documentElement?.lang==='en' && document.getElementById('game-state')?.textContent==='Ready to launch'`),'Saved language did not survive reload');
  await check('CLI language is consumed so a later selection survives reload',`document.getElementById('language-select').value==='en' && localStorage.getItem('flightdeck-language')==='en' && !new URLSearchParams(location.search).has('lang')`);
  await call('Page.navigate',{url:origin+'/?lang=de'});
  await until(()=>evaluate(queryLanguageReady),'Query language did not override saved preference');
  await check('CLI query language overrides and persists the saved language',`document.getElementById('language-select').value==='de' && localStorage.getItem('flightdeck-language')==='de'`);
  assert.ok(apiRequests.length>0 && apiRequests.every(r=>['de','en'].includes(r.language)));results.push('Every synthetic API request includes an explicit supported Accept-Language');

  status.runtime={...status.runtime,path:status.versions.msfs2020.path,game_id:'msfs2020',game_name:'Microsoft Flight Simulator 2020'};
  discovered=[{name:'Microsoft Flight Simulator 2024',game_id:'msfs2024',path:'/synthetic/msfs2024',ready:true,configured:false,checks},
    {name:'Microsoft Flight Simulator 2020',game_id:'msfs2020',path:'/synthetic/msfs2020',ready:true,configured:true,checks}];
  await route('overview');await refresh(`document.getElementById('launch-game-name').textContent==='Microsoft Flight Simulator 2020'`);
  await check('Selected 2020 version stays visible in the switch and hero',`document.getElementById('selected-game-name').textContent==='Microsoft Flight Simulator 2020' && document.getElementById('version-msfs2020').getAttribute('aria-pressed')==='true' && getComputedStyle(document.querySelector('.launch-art-2020')).backgroundImage.includes('flight-panorama-2020.png')`);
  await route('updates');await check('Updates display the selected 2020 edition',`document.getElementById('update-game-name').textContent==='Microsoft Flight Simulator 2020'`);
  await route('installation');await click('setup-discover');await until(()=>evaluate(`document.getElementById('discovery-results').textContent.includes('Microsoft Flight Simulator 2020')`),'2020 runtime was not discovered');
  await check('Both prepared editions are listed for selection',`document.getElementById('discovery-results').textContent.includes('Microsoft Flight Simulator 2024') && document.getElementById('discovery-results').textContent.includes('Microsoft Flight Simulator 2020')`);
  status.versions.msfs2024={path:'',installed:false,ready:false};
  await route('overview');await refresh(`document.getElementById('version-msfs2024-state').textContent==='Installieren'`);
  await click('version-msfs2024');
  await check('Missing edition opens its own install form in one click',`location.hash==='#installation' && document.getElementById('setup-game-id').value==='msfs2024' && !document.getElementById('setup-install-fields').hidden`);

  // Fenix: current runtime, stopped/running permissions, reservation and error text.
  status.runtime={...status.runtime,path:'/fixture/fenix-msfs2024',game_id:'msfs2024',game_name:'Microsoft Flight Simulator 2024'};
  await refresh(`document.getElementById('launch-game-name').textContent==='Microsoft Flight Simulator 2024'`);
  await route('mods');await click('fenix-refresh');
  await until(()=>evaluate(`!document.getElementById('fenix-install').disabled`),'Fenix install unavailable');
  await check('Fenix has no installer execution before a selected EXE',`document.getElementById('fenix-installer').disabled && document.getElementById('fenix-configure').disabled`);
  await check('Fenix initial step is explicit and never called ready',`document.getElementById('fenix-state').textContent==='Einrichtung noch nicht abgeschlossen' && document.getElementById('fenix-next').textContent.includes('Schritt 1 von 4') && document.getElementById('fenix-step-1').getAttribute('aria-current')==='step' && document.getElementById('fenix-overview').hidden`);
  await evaluate(`document.getElementById('fenix-card').scrollIntoView({block:'start'})`);await screenshot('fenix-setup-desktop.png');
  await click('fenix-install');await until(()=>evaluate(`document.getElementById('fenix-state').textContent.includes('läuft')`),'Fenix progress missing');
  await check('Fenix job reserves launch and all competing mutations',`document.getElementById('launch-button').disabled && document.getElementById('fenix-install').disabled && document.getElementById('fenix-restore').disabled`);
  fenix={...fenix,can_change:true,job:{state:'failed',message:'<img src=x onerror="window.fenixInjected=1"> fixture error'}};
  await click('fenix-refresh');await until(()=>evaluate(`document.getElementById('fenix-message').textContent.includes('fixture error')`),'Fenix error missing');
  await check('Fenix errors render as text without markup execution',`!window.fenixInjected && !document.querySelector('#fenix-message img')`);
  fenix={...fenix,state:'installed',installed:true,fenix_installed:true,manager_installed:true,idle:false,can_change:false,job:{state:'complete',operation:'installer',message:'Synthetic installer exited'}};
  await click('fenix-refresh');await until(()=>evaluate(`!document.getElementById('fenix-busy').hidden`),'Open Fenix window explanation missing');
  await check('An exited installer with an open app is not complete and explains disabled controls',`document.getElementById('fenix-state').textContent==='Einrichtung noch nicht abgeschlossen' && document.getElementById('fenix-next').textContent.includes('Schritt 3 von 4') && document.getElementById('fenix-configure').disabled && document.getElementById('fenix-busy').textContent.includes('Windows-Anwendung')`);
  await evaluate(`document.getElementById('fenix-card').scrollIntoView({block:'start'})`);await screenshot('fenix-waiting-desktop.png');
  fenix={...fenix,settings_ready:true,fenix_running:true,can_stop:true,job:{state:'running',operation:'open',message:'Synthetic Fenix sign-in window'}};
  status.game={...status.game,state:'external'};await refresh(`document.getElementById('game-state').textContent.length>0`);await click('fenix-refresh');
  await until(()=>evaluate(`!document.getElementById('fenix-stop').disabled`),'Stop Fenix is blocked by its own runtime lease');
  await check('Open Fenix can be stopped while setup remains reserved and the simulator is not reported as running',`!document.getElementById('fenix-app-controls').hidden && document.getElementById('fenix-stop').textContent==='Fenix beenden' && document.getElementById('fenix-configure').disabled && !document.getElementById('fenix-busy').textContent.includes('MSFS läuft')`);
  await evaluate(`document.getElementById('fenix-card').scrollIntoView({block:'start'})`);await screenshot('fenix-stop-desktop.png');
  await language('en');await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
  await evaluate(`document.getElementById('fenix-app-controls').scrollIntoView({block:'center'})`);
  await check('Stop Fenix is translated and usable on mobile',`document.documentElement.scrollWidth<=innerWidth && document.getElementById('fenix-stop').textContent==='Stop Fenix' && !document.getElementById('fenix-stop').disabled && document.getElementById('fenix-stop').getBoundingClientRect().right<=innerWidth`);await screenshot('fenix-stop-mobile.png');
  await click('fenix-stop');await until(()=>posts.at(-1)?.path==='/api/fenix/stop','Fenix stop request missing');
  await language('de');await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});
  await until(()=>evaluate(`!document.getElementById('fenix-configure').disabled`),'Final Fenix setup step unavailable');
  await check('Stopping Fenix unlocks finish setup and leaves a clear stopped control',`document.getElementById('fenix-next').textContent.includes('Schritt 4 von 4') && document.getElementById('fenix-step-4').getAttribute('aria-current')==='step' && document.getElementById('fenix-busy').hidden && !document.getElementById('fenix-app-controls').hidden && document.getElementById('fenix-stop').disabled && document.getElementById('fenix-app-state').textContent==='Fenix ist beendet'`);
  fenix={...fenix,fenix_running:true,can_stop:true,idle:false,can_change:false,job:{state:'failed',operation:'manager',message:'Synthetic WebView host crash'}};
  await click('fenix-refresh');await until(()=>evaluate(`!document.getElementById('fenix-stop').disabled`),'Orphaned WebView helper cannot be stopped');
  await check('A crashed manager still offers stop for its remaining helper processes',`!document.getElementById('fenix-app-controls').hidden && document.getElementById('fenix-manager').disabled && document.getElementById('fenix-message').textContent.includes('WebView host crash')`);
  await click('fenix-stop');await until(()=>evaluate(`!document.getElementById('fenix-manager').disabled`),'Manager restart stays blocked after helper cleanup');
  await click('fenix-configure');await until(()=>evaluate(`document.getElementById('fenix-state').textContent==='Fenix ist startbereit'`),'Fenix ready confirmation missing');
  await check('Completed Fenix setup confirms startup and shutdown without claiming account activation',`document.getElementById('fenix-next').textContent.includes('automatisch mit dem Spiel') && document.getElementById('fenix-next').textContent.includes('beim Beenden') && document.querySelectorAll('#fenix-steps [data-status=done]').length===4 && document.getElementById('fenix-step-3').textContent.includes('Lizenz prüft Fenix selbst')`);
  fenix={...fenix,update_available:true};await click('fenix-refresh');
  await until(()=>evaluate(`document.getElementById('fenix-install').textContent==='Patch aktualisieren' && !document.getElementById('fenix-install').disabled`),'Fenix update action unavailable');
  await check('An existing patch can update while retaining ready aircraft controls',`!document.getElementById('fenix-open').disabled && document.getElementById('fenix-open').parentElement.id==='fenix-app-controls' && document.getElementById('fenix-state').textContent==='Fenix ist startbereit'`);
  fenix={...fenix,update_available:false};await click('fenix-refresh');
  await until(()=>evaluate(`document.getElementById('fenix-install').disabled && document.getElementById('fenix-install').textContent==='Patch einrichten'`),'Fenix update state did not clear');
  await evaluate(`document.getElementById('fenix-card').scrollIntoView({block:'start'})`);await screenshot('fenix-ready-desktop.png');
  await click('fenix-overview');await check('Ready Fenix leads back to the normal simulator launch',`location.hash==='#overview' && !document.getElementById('view-overview').hidden`);
  await route('mods');await language('en');await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
  await evaluate(`document.getElementById('fenix-card').scrollIntoView({block:'start'})`);
  await check('Ready Fenix message is translated and fits mobile width',`document.documentElement.scrollWidth<=innerWidth && document.getElementById('fenix-state').textContent==='Fenix is ready to fly' && document.getElementById('fenix-next').textContent.includes('closes when you exit')`);await screenshot('fenix-ready-mobile.png');
  await language('de');await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});
  fenix={...fenix,state:'legacy',installed:false,manager_installed:true,job:null};await click('fenix-refresh');
  await until(()=>evaluate(`!document.getElementById('fenix-manager').disabled && !document.getElementById('fenix-legacy').hidden`),'Legacy livery manager inaccessible');
  await check('Legacy local patch offers one main Fenix action and a clearly separate installer',`document.getElementById('fenix-install').disabled && !document.getElementById('fenix-legacy').hidden && !document.getElementById('fenix-open').disabled && document.getElementById('fenix-open').getBoundingClientRect().width>0 && [...document.querySelectorAll('#fenix-card button')].filter(button=>button.textContent==='Fenix öffnen').length===1 && document.getElementById('fenix-manager').textContent==='Installer & Liveries' && document.getElementById('fenix-manager-hint').textContent.includes('separaten Fenix-Manager')`);
  status.game={...status.game,state:'running'};fenix={...fenix,fenix_running:true,can_stop:false};await refresh(`document.getElementById('game-state').textContent.length>0`);await click('fenix-refresh');
  await until(()=>evaluate(`document.getElementById('fenix-manager').disabled`),'Running simulator must block manager');
  await check('Running simulator also blocks the dedicated Fenix stop button',`!document.getElementById('fenix-app-controls').hidden && document.getElementById('fenix-stop').disabled`);
  status.game={...status.game,state:'stopped'};fenix={...fenix,state:'available',installed:false,configured:false,settings_ready:false,fenix_installed:false,manager_installed:false,fenix_running:false};await refresh(`document.getElementById('game-state').textContent.length>0`);await click('fenix-refresh');
  await language('en');await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});
  await evaluate(`document.getElementById('fenix-card').scrollIntoView({block:'start'})`);
  await check('Fenix setup is translated and fits mobile width',`document.documentElement.scrollWidth<=innerWidth && document.getElementById('fenix-install').textContent==='Install patch'`);await screenshot('fenix-setup-mobile.png');
  await language('de');

  // Launcher updates use a separate GitHub workflow, with explicit user actions.
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});
  setup.job=null;status.cloud=autoIdle();cloudData.job=null;status.game={...status.game,state:'stopped',can_start:true};
  await route('updates');await refresh(`!document.getElementById('launch-button').disabled`);
  await until(()=>evaluate(`!document.getElementById('launcher-update-check').disabled`),'Launcher update check unavailable');
  const launcherPosts=()=>posts.filter(p=>p.path.startsWith('/api/launcher-update/'));
  assert.equal(launcherPosts().length,0);results.push('Launcher never contacts GitHub automatically on startup or entering Updates');
  await check('Launcher update identity and unknown remote version are visible',`document.getElementById('launcher-update-installed').textContent==='0.1.0' && document.getElementById('launcher-update-latest').textContent==='Noch nicht geprüft'`);
  launcherWaiting=false;launcherBarrier=new Promise(resolve=>{releaseLauncher=resolve;});
  await until(()=>launcherWaiting,'Background launcher read did not begin');
  await click('launcher-update-check');launcherUnavailable=true;launcherBarrier=null;releaseLauncher();
  await until(()=>evaluate(`document.getElementById('launcher-update-error').textContent==='Synthetic launcher status unavailable'`),'Failed launcher read did not explain the error');
  assert.equal(launcherPosts().length,0);results.push('Queued launcher check cannot post after a failed in-flight status read');
  launcherUnavailable=false;
  await until(()=>evaluate(`!document.getElementById('launcher-update-check').disabled`),'Launcher did not recover after status retry');
  await click('launcher-update-check');
  await until(()=>evaluate(`document.getElementById('launcher-update-title').textContent==='Flightdeck 0.1.5 ist verfügbar' && !document.getElementById('launcher-update-install').disabled`),'Available launcher release not rendered');
  assert.equal(launcherPosts().at(-1).path,'/api/launcher-update/check');
  await evaluate(`document.getElementById('launcher-update-details').open=true`);
  await check('GitHub release notes are plain text and cannot inject markup',`document.getElementById('launcher-update-notes').textContent.includes('<img') && !document.querySelector('#launcher-update-notes img') && !window.launcherInjected`);
  await evaluate(`document.getElementById('launcher-update-card').scrollIntoView({block:'start'})`);await screenshot('launcher-update-desktop.png');
  status.game={...status.game,state:'running',can_start:false};
  await until(()=>evaluate(`document.getElementById('launcher-update-install').disabled && !document.getElementById('launcher-update-busy').hidden`),'Running game did not block launcher installation');
  await check('Running simulator still allows a read-only GitHub update check',`!document.getElementById('launcher-update-check').disabled`);
  status.game={...status.game,state:'stopped',can_start:true};
  await until(()=>evaluate(`!document.getElementById('launcher-update-install').disabled`),'Stopped game did not enable update');
  await click('launcher-update-install');
  await until(()=>evaluate(`document.getElementById('launcher-update-progress').value===42 && !document.getElementById('launcher-update-cancel').disabled`),'Launcher download progress missing');
  assert.equal(launcherPosts().at(-1).token,csrf);
  await check('Launcher install reserves Play, setup, simulator updates and game switch',`document.getElementById('launch-button').disabled && document.getElementById('version-msfs2020').disabled && document.getElementById('update-check').disabled && document.getElementById('config-button').disabled`);
  await click('launcher-update-cancel');
  await until(()=>evaluate(`!document.getElementById('launcher-update-install').disabled`),'Cancelled launcher update did not recover');
  await click('launcher-update-install');
  await until(()=>evaluate(`!document.getElementById('launcher-update-cancel').hidden`),'Second download missing');
  launcherRestartFails=true;launcherUpdate={...launcherUpdate,pending_restart:true,can_restart:true,job:{...launcherUpdate.job,state:'complete',can_cancel:false,message:'Synthetic update installed'}};
  await until(()=>evaluate(`!document.getElementById('launcher-update-restart').disabled`),'Installed update did not offer restart');
  await click('launcher-update-restart');
  await until(()=>evaluate(`document.getElementById('launcher-update-error').textContent==='Synthetic launcher restart failed' && !document.getElementById('launcher-update-restart').disabled`),'Failed launcher restart did not allow retry');
  results.push('Failed launcher restart recovers from waiting and offers an explicit retry');
  await click('launcher-update-restart');
  await until(()=>evaluate(`document.getElementById('launcher-update-installed')?.textContent==='0.1.5' && !document.getElementById('launcher-update-rollback-area')?.hidden`),'Restart did not load the installed version');
  await evaluate(`document.getElementById('launcher-update-rollback-area').open=true`);
  const beforeLauncherRollback=launcherPosts().length;
  await click('launcher-update-rollback');await check('Launcher rollback first asks inline',`!document.getElementById('launcher-update-rollback-confirm').hidden`);
  await click('launcher-update-rollback-no');assert.equal(launcherPosts().length,beforeLauncherRollback);
  await click('launcher-update-rollback');await click('launcher-update-rollback-yes');
  await until(()=>launcherPosts().length===beforeLauncherRollback+1,'Confirmed launcher rollback missing');
  assert.equal(launcherPosts().at(-1).path,'/api/launcher-update/rollback');results.push('Launcher rollback requires inline confirmation and sends no filesystem paths');
  launcherUpdate={...launcherDefault(),installed_version:'0.1.5',job:{id:'failed-fixture',state:'failed',error:'Synthetic checksum mismatch',message:'Synthetic checksum mismatch'}};
  await until(()=>evaluate(`document.getElementById('launcher-update-error').textContent==='Synthetic checksum mismatch'`),'Failed update lacks persistent explanation');
  await check('Failed launcher update stays visible and offers retry',`document.getElementById('launcher-update-check').textContent==='Erneut versuchen' && !document.getElementById('launcher-update-check').disabled`);
  await click('launcher-update-check');
  await call('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await language('en');
  await until(()=>evaluate(`document.getElementById('launcher-update-title').textContent==='Flightdeck 0.1.6 is available'`),'Launcher update did not translate');
  await evaluate(`document.getElementById('launcher-update-card').scrollIntoView({block:'start'})`);
  await check('Launcher update and Fenix link fit mobile width in English',`document.documentElement.scrollWidth<=innerWidth && document.getElementById('launcher-update-install').textContent==='Download & install' && document.querySelector('.launcher-fenix-link a').textContent==='Manage Fenix patch'`);
  await screenshot('launcher-update-mobile-en.png');
  launcherUpdate={...launcherDefault(),managed:false,unavailable_reason:'Install with the official installer first.'};
  await until(()=>evaluate(`!document.getElementById('launcher-update-unmanaged').hidden`),'Unmanaged installation explanation absent');
  await check('Source checkout explains setup without offering an install',`document.getElementById('launcher-update-install').hidden && !document.getElementById('launcher-update-check').disabled`);
  launcherUpdate=launcherDefault();await language('de');

  // Real backend and empty state; only the public release lookup is stubbed.
  const offlineBackend=`from flightdeck import launcher_update\nfrom urllib.error import URLError\ndef offline(): raise URLError('synthetic offline startup')\nlauncher_update.latest_release=offline\nfrom flightdeck.__main__ import main\nmain()`;
  realBackend=spawn('python',['-c',offlineBackend,'--state-dir',join(temp,'empty-backend-state'),'--no-browser'],{cwd:resolve(base,'..'),env:{...process.env,XDG_DATA_HOME:join(temp,'empty-data')},stdio:['ignore','pipe','pipe']});
  let backendOutput='';realBackend.stdout.on('data',chunk=>{backendOutput+=chunk;});
  await until(()=>/Flightdeck: (http:\/\/127\.0\.0\.1:\d+)/.test(backendOutput),'Real Python backend did not start');
  const backendOrigin=backendOutput.match(/Flightdeck: (http:\/\/127\.0\.0\.1:\d+)/)[1];allowedOrigins.add(backendOrigin);
  await call('Emulation.setDeviceMetricsOverride',{width:1536,height:1024,deviceScaleFactor:1,mobile:false});
  await call('Page.navigate',{url:backendOrigin+'/?lang=de'});
  await until(()=>evaluate(`document.getElementById('game-state')?.textContent === 'Installation noch nicht verbunden'`),'Real service status/CSP did not render');
  await until(()=>evaluate(`document.fonts.check('16px Manrope')`),'Local font did not load');
  await check('Local Manrope font loads without a third-party request',`document.fonts.check('16px Manrope')`);
  await check('Real Python backend loads every local module under CSP',`document.getElementById('connection').classList.contains('online') && document.getElementById('launch-label').textContent==='Installation einrichten'`);
  await click('launch-button');await until(()=>evaluate(`!document.querySelector('input[name=setup_mode][value=existing]').disabled`),'Real setup mode unavailable');await evaluate(`document.querySelector('input[name=setup_mode][value=existing]').click()`);await until(()=>evaluate(`!document.getElementById('runtime-path').disabled`),'Real setup form unavailable');await check('Real unconfigured backend offers setup only',`location.hash === '#installation' && !document.getElementById('runtime-path').disabled`);
  await screenshot('installation-real-backend.png');
  await route('mods');await until(()=>evaluate(`!document.getElementById('mods-setup').hidden`),'Real unconfigured Community state missing');
  await check('Real empty backend offers setup, no invented Community folder or mods',`document.getElementById('mods-open').disabled && document.getElementById('mods-inventory').hidden && document.getElementById('mods-folder-row').hidden`);

  await route('updates');await until(()=>evaluate(`!document.getElementById('update-setup').hidden && document.getElementById('update-title').textContent==='Updates derzeit nicht verfügbar'`),'Real unconfigured update state missing');
  await check('Real empty backend never invents an installed or available game version',`document.getElementById('update-installed').textContent==='Nicht bekannt' && document.getElementById('update-latest').textContent==='Noch nicht geprüft' && document.getElementById('update-check').disabled && document.getElementById('update-start').hidden && document.getElementById('update-sign-in').hidden`);

  await route('diagnostics');await click('diagnostic-refresh');
  await until(()=>evaluate(`!document.getElementById('diagnostic-download').disabled`),'Real safe diagnostics failed');
  await check('Real diagnostics render without token fields',`!document.getElementById('diagnostic-json').textContent.includes('csrf_token') && document.getElementById('diagnostic-summary').textContent.includes('Spielsitzung gefunden')`);
  assert.deepEqual(errors,[]);assert.deepEqual(consoleIssues,[]);assert.deepEqual(externalRequests,[]);results.push('No runtime exceptions, console warnings/errors or external network requests');
  await writeFile(join(artifacts,'browser-results.json'),JSON.stringify({passed:results.length,checks:results,method:'Isolated Chromium CDP, synthetic API mutations and real empty backend reads',viewport:{desktop:[1536,1024],mobile:[390,844]},realRuntimeActions:false},null,2)+'\n');
  console.log(`PASS ${results.length} browser checks; screenshots: ${artifacts}`);
} catch(error) {
  console.error(error.stack);console.error(chromeErrors.slice(-2000));process.exitCode=1;
} finally {
  releaseLauncher?.();releaseCloud?.();releaseUpdate?.();releaseStatus?.();socket?.close();chrome.kill('SIGTERM');realBackend?.kill('SIGTERM');
  await new Promise(resolve=>server.close(resolve));
  await sleep(250);await rm(temp,{recursive:true,force:true,maxRetries:5,retryDelay:100});
}
