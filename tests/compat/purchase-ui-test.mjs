// SPDX-License-Identifier: MIT
// Offline browser test of the production checkout host. Every HTTP request is
// intercepted; the Microsoft frame is synthetic and no account is accessed.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {mkdtemp,readFile,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';

const stage=resolve(process.argv[2]??'build/compat');
const template=await readFile(join(stage,'xodus-src/crates/xodus-cli/src/store_purchase.html'),'utf8');
const temp=await mkdtemp(join(tmpdir(),'flightdeck-purchase-ui-'));
const config={productId:'ABCD1234EFGH',skuId:'0001',availabilityId:'AVAIL1234567',market:'AT',locale:'en-US',xToken:'XBL3.0 x=0;SYNTHETIC-NOT-A-CREDENTIAL',expiresAt:4102444800};
const html=template.replaceAll('__NONCE__','a'.repeat(32)).replace('__MODE__','checkout').replace('__CONFIG__',JSON.stringify(config));
const url='https://www.microsoft.com/store/purchase/buynowui/buynow';
const hostUrl='https://www.microsoft.com/store/purchase/buynowui/prefetch/buynow';
const browser=spawn(process.env.CHROMIUM_BIN??'chromium',['--headless=new','--no-sandbox','--disable-gpu','--disable-dev-shm-usage','--no-first-run','--disable-background-networking','--disable-component-update','--disable-sync','--disable-extensions','--disable-site-isolation-trials','--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE localhost','--remote-debugging-port=0',`--user-data-dir=${join(temp,'profile')}`,'about:blank'],{stdio:['ignore','ignore','ignore']});
let socket,serial=0,requests=[],fields,results=[],contexts=new Map(),fatal,hostHtml=html;
const pending=new Map();
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,message){const end=Date.now()+20000;while(Date.now()<end){if(fatal)throw fatal;if(await fn())return;await sleep(30);}throw new Error(message);}
const call=(method,params={})=>new Promise((resolve,reject)=>{const id=++serial;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params}));});
async function evaluate(expression,contextId){const r=await call('Runtime.evaluate',{expression,contextId,returnByValue:true,awaitPromise:true});if(r.exceptionDetails)throw new Error(r.exceptionDetails.text);return r.result.value;}
function check(name,ok){assert.ok(ok,name);results.push(name);}
try{
  let port;await until(async()=>{try{port=(await readFile(join(temp,'profile/DevToolsActivePort'),'utf8')).split('\n')[0];return !!port;}catch{return false;}},'browser start');
  const tabs=await(await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  socket=new WebSocket(tabs.find(t=>t.type==='page').webSocketDebuggerUrl);
  await new Promise((r,j)=>{socket.addEventListener('open',r,{once:true});socket.addEventListener('error',j,{once:true});});
  socket.addEventListener('message',async event=>{
    const data=JSON.parse(event.data);
    if(data.id){const p=pending.get(data.id);pending.delete(data.id);if(data.error)p.reject(new Error(JSON.stringify(data.error)));else p.resolve(data.result);}
    if(data.method==='Runtime.executionContextCreated')contexts.set(data.params.context.id,data.params.context);
    if(data.method==='Runtime.executionContextsCleared')contexts.clear();
    if(data.method==='Runtime.executionContextDestroyed')contexts.delete(data.params.executionContextId);
    if(data.method==='Fetch.requestPaused'){
      try{
        const {requestId,request}=data.params;requests.push({url:request.url,method:request.method});
        if(request.url===hostUrl){
          await call('Fetch.fulfillRequest',{requestId,responseCode:200,responseHeaders:[{name:'Content-Type',value:'text/html'}],body:Buffer.from(hostHtml).toString('base64')});
        }else if(request.url.startsWith(url+'?')){
          fields=new URLSearchParams(request.postData);
          const body=`<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><style>
            html,body{margin:0;height:100%;background:#242424;color:#fff;font:16px system-ui}*{box-sizing:border-box}
            main{height:100%;display:flex;flex-direction:column;padding:24px}section{flex:1;min-height:0;overflow:auto}h1{font-size:24px;margin-top:0}
            footer{flex:none;padding-top:18px}p{line-height:1.5}nav{display:flex;gap:12px;flex-wrap:wrap}button{padding:12px 20px}
            </style><title>Offline Store preview</title></head><body><main><section><h1>Microsoft Store test fixture</h1><p>No account. No payment service.</p></section>
            <footer><p id="terms">This is an offline layout preview. Its confirmation controls do not place orders or charge a payment method. Long notices must remain separate from the controls.</p>
            <nav><button id="fixture-cancel">Cancel preview</button><button id="fixture-confirm">Test confirmation</button></nav></footer></main></body></html>`;
          await call('Fetch.fulfillRequest',{requestId,responseCode:200,responseHeaders:[{name:'Content-Type',value:'text/html'}],body:Buffer.from(body).toString('base64')});
        }else await call('Fetch.failRequest',{requestId,errorReason:'BlockedByClient'});
      }catch(e){fatal=e;}
    }
  });
  await call('Page.enable');await call('Runtime.enable');
  await call('Fetch.enable',{patterns:[{urlPattern:'http*://*',requestStage:'Request'}]});
  await call('Page.addScriptToEvaluateOnNewDocument',{source:'window.__messages=[]; window.ipc={postMessage:m=>window.__messages.push(JSON.parse(m))}; window.__timers={}; const originalTimeout=window.setTimeout; window.setTimeout=(f,ms,...args)=>{if(ms>=45000)window.__timers[ms]=f; return originalTimeout(f,ms,...args);};'});
  await call('Emulation.setDeviceMetricsOverride',{width:720,height:820,deviceScaleFactor:1,mobile:false});
  const navigate=async()=>{
    fields=null;await call('Page.navigate',{url:hostUrl});
    await until(()=>fields!==null,'intercept confirmation form');
    try { await until(()=>[...contexts.values()].filter(c=>c.origin==='https://www.microsoft.com'&&c.auxData?.isDefault).length===2,'synthetic checkout frame'); } catch(e) { await writeFile(join(temp,'contexts.json'),JSON.stringify({contexts:[...contexts.values()],tree:await call('Page.getFrameTree')})); console.error(temp); throw e; }
  };
  const checkoutFrame=()=>[...contexts.values()].filter(c=>c.origin==='https://www.microsoft.com'&&c.auxData?.isDefault).at(-1).id;
  await navigate();
  check('opens confirmation with POST',requests[1].method==='POST');
  check('exact product SKU and offer',fields.get('products')===JSON.stringify([{productId:config.productId,skuId:config.skuId,availabilityId:config.availabilityId}]));
  check('requests Microsoft hosted Xbox layout',fields.get('cssOverride')==='XboxCom2NewUI'&&fields.get('flights[0]')==='sc_disabledefaultstyles');
  check('bound token stays in POST body',fields.get('xToken')===config.xToken&&!requests[1].url.includes(config.xToken));
  check('IPC nonce is not sent as correlation data',fields.get('cV')!==Buffer.from('a'.repeat(32),'hex').toString('base64').replace(/=+$/,'')+'.0');
  check('opening is not payment success',await evaluate('window.__messages.filter(m=>m.status).length===0'));
  await evaluate(`window.dispatchEvent(new MessageEvent('message',{origin:'https://attacker.invalid',source:document.getElementById('checkout').contentWindow,data:{message:'done',status:'success',orderId:'fake'}}))`);
  await evaluate(`window.dispatchEvent(new MessageEvent('message',{origin:'https://www.microsoft.com',source:window,data:{message:'done',status:'success',orderId:'fake'}}))`);
  check('wrong origin and wrong frame ignored',await evaluate('window.__messages.filter(m=>m.status).length===0'));
  let frame=checkoutFrame();
  await evaluate(`parent.postMessage({message:'stageChanged',stage:'ready'},'*')`,frame);
  await until(()=>evaluate(`document.getElementById('status').textContent==='Microsoft Store'`),'ready state');
  check('ready is not payment success',await evaluate('window.__messages.filter(m=>m.status).length===0'));
  check('ready header gives space to the confirmation page',await evaluate("document.querySelector('header').getBoundingClientRect().height<64 && getComputedStyle(document.getElementById('detail')).display==='none'"));
  await evaluate(`parent.postMessage({message:'sizeChanged',height:520},'*')`,frame);
  await until(()=>evaluate("document.getElementById('checkout').style.getPropertyValue('--checkout-height')==='520px'"),'trusted frame resize');
  check('trusted height is applied within the window',await evaluate("document.getElementById('checkout').getBoundingClientRect().height===520"));
  for(const height of [-1,0,100000,NaN])await evaluate(`parent.postMessage({message:'sizeChanged',height:${String(height)}},'*')`,frame);
  await evaluate("window.dispatchEvent(new MessageEvent('message',{origin:'https://attacker.invalid',source:document.getElementById('checkout').contentWindow,data:{message:'sizeChanged',height:600}}))");
  await sleep(30);check('invalid or untrusted dimensions ignored',await evaluate("document.getElementById('checkout').style.getPropertyValue('--checkout-height')==='520px'"));
  await evaluate(`parent.postMessage({message:'sizeChanged',height:640},'*')`,frame);
  await until(()=>evaluate("document.getElementById('checkout').style.getPropertyValue('--checkout-height')==='640px'"),'normal frame height');
  for(const [width,height,label] of [[720,820,'desktop'],[1100,1400,'tall'],[360,640,'narrow'],[640,480,'short']]){
    await call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});
    await sleep(60);
    check(`host controls and frame fit ${label} window`,await evaluate(`(()=>{const f=document.getElementById('checkout').getBoundingClientRect(),b=document.getElementById('cancel').getBoundingClientRect();return f.width<=720 && f.height<=641 && f.top>=document.querySelector('header').getBoundingClientRect().bottom && f.bottom<=b.top && b.bottom<=innerHeight && f.left>=0 && f.right<=innerWidth})()`));
    check(`fixture terms and controls stay separate ${label}`,await evaluate("document.getElementById('terms').getBoundingClientRect().bottom<=document.querySelector('nav').getBoundingClientRect().top",frame));
    const screenshot=await call('Page.captureScreenshot',{format:'png'});await writeFile(join(temp,`layout-${label}.png`),Buffer.from(screenshot.data,'base64'));
  }
  await call('Emulation.setDeviceMetricsOverride',{width:720,height:820,deviceScaleFactor:1,mobile:false});
  await evaluate(`parent.postMessage({message:'done',status:'success',orderId:'synthetic-order-1'},'*')`,frame);
  await until(()=>evaluate('window.__messages.filter(m=>m.status).length===1'),'confirmed result');
  check('official-frame completion relayed',await evaluate(`window.__messages.find(m=>m.status).status==='success'&&window.__messages.find(m=>m.status).orderId==='synthetic-order-1'`));
  await evaluate(`parent.postMessage({message:'done',status:'success',orderId:'synthetic-order-2'},'*')`,frame);
  await sleep(40);check('duplicate completion ignored',await evaluate('window.__messages.filter(m=>m.status).length===1'));
  await navigate();
  await evaluate(`document.getElementById('cancel').click()`);
  check('host cancellation is distinct',await evaluate(`window.__messages.filter(m=>m.status).length===1&&window.__messages.find(m=>m.status).status==='cancel'`));
  const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(join(temp,'checkout.png'),Buffer.from(shot.data,'base64'));
  await navigate();
  frame=checkoutFrame();
  await evaluate(`parent.postMessage({message:'done',status:'success'},'*')`,frame);
  await until(()=>evaluate("document.getElementById('main').classList.contains('failed')"),'visible error');
  check('malformed completion stays visible until dismissed',await evaluate("window.__messages.filter(m=>m.status).length===0 && document.getElementById('detail').textContent.includes('order history')"));
  await evaluate("document.getElementById('cancel').click()");
  check('missing order cannot succeed or trap cancellation',await evaluate(`window.__messages.find(m=>m.status).status==='error'`));
  await navigate();
  await evaluate('window.__timers[45000]()');
  check('early timeout is visible and stops the embedded page',await evaluate("document.getElementById('checkout').hidden && document.getElementById('detail').textContent.includes('45 seconds') && window.__messages.some(m=>m.phase==='load_timeout')"));
  const failureShot=await call('Page.captureScreenshot',{format:'png'});await writeFile(join(temp,'load-timeout.png'),Buffer.from(failureShot.data,'base64'));
  await evaluate("document.getElementById('cancel').click()");
  check('timeout is distinct from user cancellation',await evaluate("window.__messages.find(m=>m.status).status==='load_timeout'"));
  await navigate();
  await evaluate('window.__timers[2147483647]()');
  check('expired session has actionable message',await evaluate("document.getElementById('detail').textContent.includes('sign in again')"));
  await evaluate("document.getElementById('cancel').click()");
  check('expired result stays distinct',await evaluate("window.__messages.find(m=>m.status).status==='expired'"));
  await navigate();
  await evaluate('window.__timers[900000]()');
  check('long session timeout does not claim sign-in expired',await evaluate("document.getElementById('detail').textContent.includes('15 minutes') && window.__messages.some(m=>m.phase==='session_timeout')"));
  check('only synthetic host and confirmation requests',requests.length===12&&requests.every(r=>r.url===hostUrl||r.url.startsWith(url+'?')));
  let posted=requests.filter(r=>r.method==='POST').length;
  await call('Page.navigate',{url:'data:text/html;base64,'+Buffer.from(html).toString('base64')});
  await until(()=>evaluate("window.__messages.some(m=>m.phase==='bootstrap_error')"),'opaque origin rejected');
  check('opaque origin cannot submit confirmation',requests.filter(r=>r.method==='POST').length===posted);
  check('bootstrap failure is visible',await evaluate("document.getElementById('checkout').hidden && document.getElementById('detail').textContent.includes('connection')"));
  await evaluate("document.getElementById('cancel').click()");
  check('bootstrap failure remains an error on close',await evaluate("window.__messages.find(m=>m.status).status==='bootstrap_error'"));
  hostHtml=html.replace("const mode = 'checkout'", "const mode = 'bootstrap'");
  await call('Page.navigate',{url:hostUrl});
  await until(()=>evaluate("window.__messages.some(m=>m.phase==='bootstrap_started')"),'bootstrap opening view');
  check('bootstrap sends neither token nor confirmation',requests.filter(r=>r.method==='POST').length===posted);
  await evaluate("document.getElementById('cancel').click()");
  check('bootstrap can be cancelled before navigation',await evaluate("window.__messages.find(m=>m.status).status==='cancel'"));
  hostHtml=html.replace('"expiresAt":4102444800','"expiresAt":1');
  await call('Page.navigate',{url:hostUrl});
  await until(()=>evaluate("window.__messages.some(m=>m.phase==='expired')"),'expiry before submission');
  check('token expired during bootstrap is never submitted',requests.filter(r=>r.method==='POST').length===posted);
  await writeFile(join(temp,'result.json'),JSON.stringify({passed:true,checks:results,real_account_calls:0,real_payment_requests:0},null,2));
  console.log(JSON.stringify({passed:true,checks:results.length,artifacts:temp}));
}finally{
  socket?.close();browser.kill();
  // Keep the report/screenshot, remove only this test's disposable browser data.
  await sleep(100);await rm(join(temp,'profile'),{recursive:true,force:true,maxRetries:3});
}
