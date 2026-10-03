import {t} from './i18n.js';

export function protonActions(data,status,{online=true,pending=false,reserved=false}={}) {
  const busy=data?.job?.state==='preparing';
  const bound=!!status?.runtime.configured&&data?.runtime_path===status.runtime.path;
  const idle=bound&&online&&!pending&&!reserved&&!busy&&!status.setup?.busy&&status.game.state==='stopped';
  return {select:idle&&!data.error&&!data.fenix&&!['syncing','playing','attention'].includes(status.cloud?.state),
    restore:idle&&data.can_restore===true&&!['syncing','playing'].includes(status.cloud?.state),
    cancel:online&&!pending&&busy&&data.job.runtime_path===status?.runtime.path,busy};
}

export function createProton({request,getStatus,isOnline,isReserved,refreshStatus,changed}) {
  const $=id=>document.getElementById(id);
  let data=null,pending=false,loading=null,error='',reserved=false,boundPath=null;
  function render() {
    const status=getStatus(),allowed=protonActions(data,status,{online:isOnline(),pending,reserved:isReserved()});
    const bound=!!data&&!!status?.runtime.configured&&data.runtime_path===status.runtime.path;
    const next=pending||allowed.busy;
    if(next!==reserved){reserved=next;changed(next);}
    $('proton-card').hidden=!status?.runtime.configured;
    $('proton-current').textContent=bound?data.selected:'—';
    $('proton-select').disabled=!allowed.select;
    for(const option of $('proton-select').options){
      if(option.value==='')option.text=t('Proton-Version auswählen …');
      if(option.value==='custom')option.text=t('Anderen Proton-Ordner wählen');
    }
    $('proton-path').disabled=!allowed.select;
    $('proton-path-field').hidden=$('proton-select').value!=='custom';
    $('proton-apply').disabled=!allowed.select||!choice();
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
        if(boundPath!==path){boundPath=path;await discover();}
      }catch(problem){error=problem.message;}
      finally{loading=null;render();}
    })();return loading;
  }
  async function discover() {
    try{
      const result=await request('/api/proton/discover'),previous=$('proton-select').value;
      const options=[new Option(t('Proton-Version auswählen …'),'')];
      for(const item of result.choices||[])options.push(new Option(`${item.label} · ${item.version}`,item.path));
      options.push(new Option(t('Anderen Proton-Ordner wählen'),'custom'));
      $('proton-select').replaceChildren(...options);
      if(options.some(item=>item.value===previous))$('proton-select').value=previous;
    }catch(problem){error=problem.message;}
    render();
  }
  async function action(mode) {
    const allowed=protonActions(data,getStatus(),{online:isOnline(),pending,reserved:isReserved()});
    if(!(mode==='default'?allowed.restore:allowed.select))return;
    pending=true;error='';render();
    try{await request('/api/proton/select',{method:'POST',token:getStatus().csrf_token,body:{runtime_path:getStatus().runtime.path,mode,path:choice()}});await load();}
    catch(problem){error=problem.message;}
    finally{pending=false;await refreshStatus();render();}
  }
  $('proton-select').addEventListener('change',render);
  $('proton-path').addEventListener('input',render);
  $('proton-discover').addEventListener('click',()=>void discover());
  $('proton-apply').addEventListener('click',()=>void action('proton'));
  $('proton-restore').addEventListener('click',()=>void action('default'));
  $('proton-cancel').addEventListener('click',async()=>{
    if(!protonActions(data,getStatus(),{online:isOnline(),pending}).cancel)return;
    pending=true;render();
    try{await request('/api/proton/cancel',{method:'POST',token:getStatus().csrf_token,body:{job_id:data.job.id}});}
    catch(problem){error=problem.message;}
    finally{pending=false;await load();render();}
  });
  return {render,load,poll:()=>{if(reserved||location.hash==='#installation')void load();}};
}
