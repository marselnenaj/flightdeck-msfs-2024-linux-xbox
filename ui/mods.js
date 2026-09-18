import {t, getLanguage} from './i18n.js';
import {stringValue, formatCount} from './state.js';

const STATES=new Set(['ready','unconfigured','unknown','missing','ambiguous','error']);
const ITEM_STATES=new Set(['available','invalid_manifest','missing_manifest','unreadable']);
const count=value=>Number.isSafeInteger(value)&&value>=0?value:null;

export function normalizeMods(raw) {
  if(!raw||!STATES.has(raw.state)||!Array.isArray(raw.mods))throw new Error(t('Der Add-on-Bestand konnte nicht gelesen werden.'));
  const mods=raw.mods.slice(0,1000).map(item=>{
    if(!item||typeof item.id!=='string'||!item.id||!ITEM_STATES.has(item.status))throw new Error(t('Der Add-on-Bestand konnte nicht gelesen werden.'));
    return {id:stringValue(item.id,'',255),name:stringValue(item.name,'',240),version:stringValue(item.version,'',100),
      creator:stringValue(item.creator,'',180),status:item.status,is_link:item.is_link===true};
  });
  const folder=stringValue(raw.folder_path);
  return {state:raw.state,message:stringValue(raw.message,'',1200),folder_path:folder.startsWith('/')?folder:'',
    can_open:raw.can_open===true&&raw.state==='ready'&&folder.startsWith('/'),mods,count:count(raw.count),
    limited:raw.limited!==false||count(raw.count)===null||raw.mods.length>1000||raw.count>mods.length};
}

export function modsMessage(data) {
  if(!data)return t('Community-Ordner wird gesucht …');
  if(data.message)return data.message;
  const messages={unconfigured:'Richte zuerst deine Spielinstallation in Flightdeck ein.',
    unknown:'Prüfe den im Simulator verwendeten Community-Ordner und aktualisiere die Liste.',
    missing:'Der ermittelte Community-Ordner fehlt. Prüfe den Speicherort deiner Add-ons im Simulator.',
    ambiguous:'Es wurden mehrere mögliche Community-Ordner gefunden. Prüfe den aktiven Paketordner im Simulator.',
    error:'Der Community-Ordner ist nicht lesbar. Prüfe die Zugriffsrechte und versuche es erneut.'};
  return data.state==='ready'?t('Der Community-Ordner wurde gefunden.'):t(messages[data.state]);
}

export function createMods({request,getStatus,isOnline,isReserved,notice}) {
  const $=id=>document.getElementById(id);
  let data=null,loading=false,opening=false,error='',fresh=false,renderedData,renderedLanguage,loadedRuntime;
  const current=()=>fresh&&(!loadedRuntime||loadedRuntime===getStatus()?.runtime.path);
  function render() {
    $('mods-refresh').disabled=loading;
    $('mods-refresh-label').textContent=t(loading?'Liste wird geladen …':'Aktualisieren');
    $('mods-open').disabled=!current()||!data?.can_open||loading||opening||!isOnline()||isReserved();
    $('mods-open-label').textContent=t(opening?'Ordner wird geöffnet …':'Community-Ordner öffnen');
    $('mods-message').textContent=loading&&!data?t('Community-Ordner wird gesucht …'):modsMessage(data);
    $('mods-error').textContent=error;$('mods-error').hidden=!error;
    $('mods-stale').hidden=!data||current()||loading;
    $('mods-folder').textContent=data?.folder_path||'';$('mods-folder-row').hidden=!data?.folder_path;
    $('mods-setup').hidden=data?.state!=='unconfigured';
    const ready=data?.state==='ready';
    $('mods-inventory').hidden=!ready;
    $('mods-count').textContent=data?.count===null||!data?t('Community-Inhalte'):t('Gefundene Ordner: {count}',{count:formatCount(data.count)});
    $('mods-limited').hidden=!ready||!data.limited;
    $('mods-empty').hidden=!ready||data.mods.length>0;
    $('mods-empty').textContent=t(data?.limited?'Im geprüften Teil wurden keine Add-on-Ordner gefunden.':'Noch keine Add-ons in diesem Community-Ordner.');
    $('mods-empty-help').hidden=!ready||data.mods.length>0||data.limited;
    if(data===renderedData&&getLanguage()===renderedLanguage)return;
    renderedData=data;renderedLanguage=getLanguage();
    const rows=(data?.mods||[]).map(item=>{
      const row=document.createElement('li');row.className='mod-row';
      const main=document.createElement('div');main.className='mod-main';
      const name=document.createElement('h3');name.textContent=item.name||item.id;main.append(name);
      if(item.creator){const creator=document.createElement('p');creator.textContent=item.creator;main.append(creator);}
      if(item.name&&item.name!==item.id){const folder=document.createElement('code');folder.textContent=item.id;main.append(folder);}
      const version=document.createElement('dl');version.className='mod-version';
      const term=document.createElement('dt');term.textContent=t('Version');
      const value=document.createElement('dd');value.textContent=item.version||t('Nicht angegeben');version.append(term,value);
      const detail=document.createElement('p');detail.className='mod-detail';
      const statuses={invalid_manifest:'Manifest ungültig',missing_manifest:'manifest.json fehlt',unreadable:'Manifest nicht lesbar'};
      detail.textContent=item.status==='available'?(item.is_link?t('Verknüpfter Ordner'):''):t(statuses[item.status]);
      detail.hidden=!detail.textContent;
      row.append(main,version,detail);return row;
    });
    $('mods-list').replaceChildren(...rows);
  }
  async function load() {
    if(loading)return;
    loading=true;error='';render();
    const requestedRuntime=getStatus()?.runtime.path;
    try{data=normalizeMods(await request('/api/mods'));loadedRuntime=requestedRuntime;fresh=true;}
    catch(failure){error=failure.message;fresh=false;}
    finally{loading=false;render();}
  }
  async function openFolder() {
    if(!current()||!data?.can_open||loading||opening||!isOnline()||isReserved()||!getStatus()?.csrf_token)return;
    opening=true;render();
    try{const result=await request('/api/mods/open-folder',{method:'POST',body:{},token:getStatus().csrf_token});notice(stringValue(result.message,t('Der Community-Ordner wird im Dateimanager geöffnet.'),1200));}
    catch(failure){notice(failure.message,true);}
    finally{opening=false;await load();}
  }
  $('mods-refresh').addEventListener('click',()=>void load());
  $('mods-open').addEventListener('click',()=>void openFolder());
  render();
  return {load,render};
}
