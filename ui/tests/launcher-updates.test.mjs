import test from 'node:test';
import assert from 'node:assert/strict';
import {normalizeLauncherUpdate,launcherUpdateTitle} from '../launcher-updates.js';
import {setLanguage} from '../i18n.js';

test('missing or unknown launcher status cannot claim an update or grant capabilities',()=>{
  assert.throws(()=>normalizeLauncherUpdate(null));assert.throws(()=>normalizeLauncherUpdate({}));
  const data=normalizeLauncherUpdate({managed:false,installed_version:'0.1.4',can_install:'true',update_available:'false'});
  assert.equal(data.can_install,false);assert.equal(data.update_available,null);
  setLanguage('en');assert.equal(launcherUpdateTitle(data),'Updates have not been checked yet');
  data.update_available=false;assert.equal(launcherUpdateTitle(data),'Flightdeck is up to date');setLanguage('de');
});
test('progress is bounded and running/failed/restart states have distinct titles',()=>{
  setLanguage('en');
  const data=normalizeLauncherUpdate({managed:true,installed_version:'0.1.4',latest_version:'0.1.5',update_available:true,
    job:{id:'fixture',operation:'install',state:'running',phase:'verifying',progress:125,total:Infinity,received:-1}});
  assert.equal(data.job.progress,100);assert.equal(data.job.total,null);assert.equal(data.job.received,0);
  assert.equal(launcherUpdateTitle(data),'Verifying download …');
  data.job.state='failed';assert.equal(launcherUpdateTitle(data),'Update not completed');
  data.pending_restart=true;assert.equal(launcherUpdateTitle(data),'Ready to restart');
  data.pending_restart=false;data.job=null;assert.equal(launcherUpdateTitle(data),'Flightdeck 0.1.5 is available');setLanguage('de');
});
