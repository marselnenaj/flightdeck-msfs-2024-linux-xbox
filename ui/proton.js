import {t} from './i18n.js';

export function protonActions(data,status,{online=true,pending=false,reserved=false}={}) {
  const busy=data?.job?.state==='preparing';
  const bound=!!status?.runtime.configured&&data?.runtime_path===status.runtime.path;
  const idle=bound&&online&&!pending&&!reserved&&!busy&&!status.setup?.busy&&status.game.state==='stopped';
  return {select:idle&&!data.error&&!['syncing','playing','attention'].includes(status.cloud?.state),
    restore:idle&&data.can_restore===true&&!['syncing','playing'].includes(status.cloud?.state),
    cancel:online&&!pending&&busy&&data.job.runtime_path===status?.runtime.path,busy};
}

export function createProton({request,polling,getStatus,isOnline,isReserved,isSetupActive=()=>false,refreshStatus,changed}) {
  const $=id=>document.getElementById(id);
  let data=null,pending=false,loading=null,error='',reserved=false,boundPath=null;
  function render() {
    const status=getStatus(),allowed=protonActions(data,status,{online:isOnline(),pending,reserved:isReserved()});
    const bound=!!data&&!!status?.runtime.configured&&data.runtime_path===status.runtime.path;
    const next=pending||allowed.busy;
    if(next!==reserved){reserved=next;changed(next);}
    $('proton-card').hidden=!status?.runtime.configured||isSetupActive();
    $('proton-current').textContent=bound?data.selected:'—';
    $('proton-compact-state').removeAttribute('data-i18n');
    $('proton-compact-state').textContent=bound?(data.error||(allowed.busy?data.job.message:data.selected)):t('Andere Proton-Version testen');
    $('proton-select').disabled=!allowed.select&&!allowed.restore;
    for(const option of $('proton-select').options){
      if(option.value==='')option.text=t('Proton-Version auswählen …');
      if(option.value==='default')option.text=t('Flightdeck (Xodus, Standard)');
      if(option.value==='custom'){option.text=t('Anderen Proton-Ordner wählen');option.disabled=!allowed.select;}
      if(option.dataset.label){
        const unsupported=!!data?.fenix&&option.dataset.fenix!=='true';
        option.disabled=!allowed.select||unsupported;
        option.text=option.dataset.label+(data?.fenix?' · '+t(unsupported?'Fenix-Patch fehlt':'Fenix verfügbar'):'');
      }
    }
    $('proton-path').disabled=!allowed.select;
    $('proton-path-field').hidden=$('proton-select').value!=='custom';
    $('proton-apply').disabled=$('proton-select').value==='default'?!allowed.restore:
      !allowed.select||!choice()||$('proton-select').selectedOptions[0]?.disabled===true;
    $('proton-discover').disabled=!allowed.select;
    $('proton-restore').hidden=!data?.can_restore;
    $('proton-restore').disabled=!allowed.restore;
    $('proton-cancel').hidden=!allowed.busy;
    $('proton-cancel').disabled=!allowed.cancel;
    $('proton-progress').hidden=!allowed.busy;
    $('proton-fenix').hidden=!data?.fenix;
    $('proton-message').textContent=bound&&data.job?.runtime_path===status.runtime.path?data.job.message||'':'';
    const problem=error||data?.error||(bound&&data.job?.runtime_path===status.runtime.path?data.job.error:'')||'';
    $('proton-error').textContent=problem;$('proton-error').hidden=!problem;
  }
  function choice(){return $('proton-select').value==='custom'?$('proton-path').value.trim():$('proton-select').value;}
  async function load() {
    if(loading)return loading;
    const path=getStatus()?.runtime.path;
    loading=(async()=>{
      try {
        const result=await request('/api/proton');
        if(path!==getStatus()?.runtime.path)return;
        data=result;error='';
        if(boundPath!==path){boundPath=path;await discover(true);}
      }catch(problem){error=problem.message;}
      finally{loading=null;render();}
    })();return loading;
  }
  async function discover(initial=false) {
    try{
      const result=await request('/api/proton/discover'),previous=$('proton-select').value;
      const options=[new Option(t('Proton-Version auswählen …'),''),new Option(t('Flightdeck (Xodus, Standard)'),'default')];
      for(const item of result.choices||[]){
        const option=new Option(`${item.label} · ${item.version}`,item.path);
        option.dataset.label=option.text;option.dataset.fenix=String(item.fenix===true);options.push(option);
      }
      options.push(new Option(t('Anderen Proton-Ordner wählen'),'custom'));
      $('proton-select').replaceChildren(...options);
      const selected=initial||!options.some(item=>item.value===previous)
        ? (data?.experimental?(result.choices||[]).find(item=>item.version===data.selected)?.path||'':'default')
        : previous;
      $('proton-select').value=selected;
    }catch(problem){error=problem.message;}
    render();
  }
  async function action(mode) {
    const allowed=protonActions(data,getStatus(),{online:isOnline(),pending,reserved:isReserved()});
    if(!(mode==='default'?allowed.restore:allowed.select))return;
    pending=true;error='';render();
    try{
      await request('/api/proton/select',{method:'POST',token:getStatus().csrf_token,body:{runtime_path:getStatus().runtime.path,mode,path:mode==='default'?'':choice()}});
      if(mode==='default')$('proton-select').value='default';
      await load();
    }
    catch(problem){error=problem.message;}
    finally{pending=false;await refreshStatus();render();}
  }
  $('proton-select').addEventListener('change',render);
  $('proton-path').addEventListener('input',render);
  $('proton-discover').addEventListener('click',()=>void discover());
  $('proton-apply').addEventListener('click',()=>void action($('proton-select').value==='default'?'default':'proton'));
  $('proton-restore').addEventListener('click',()=>void action('default'));
  $('proton-cancel').addEventListener('click',async()=>{
    if(!protonActions(data,getStatus(),{online:isOnline(),pending}).cancel)return;
    pending=true;render();
    try{await request('/api/proton/cancel',{method:'POST',token:getStatus().csrf_token,body:{job_id:data.job.id}});}
    catch(problem){error=problem.message;}
    finally{pending=false;await load();render();}
  });
  polling.add('proton', load, () => pending ? null : reserved ? 1500 : location.hash === '#installation' ? 10000 : null);
  return {render,load};
}
