import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';
// llmlint: ignore-block[shell_test_tiers_stay_split] The required offline real Git/Python workflow suite runs in github-workflows:test, measured about 10 seconds, with no installs or model calls; artifact installation remains in the separate integration tier.
test('real release builder rejects malformed platform declarations and package-directory boundary rejects defective staging',()=>{
 const cwd=mkdtempSync(join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR??tmpdir(),'payload-boundary-'));
 try{
  for(const document of ['broken [', '[[platform]]\ntarget="invalid"\nnpm="darwin-x64"\narchive="tar.gz"', '[[platform]]\ntarget="x86_64-apple-darwin"\nnpm="darwin-arm64"\narchive="tar.gz"', '[[platform]]\ntarget="x86_64-apple-darwin"\nnpm="darwin-x64"\narchive="zip"', ('[[platform]]\ntarget="x86_64-apple-darwin"\nnpm="darwin-x64"\narchive="tar.gz"\n').repeat(2)]){
   writeFileSync(join(cwd,'release-platforms.toml'),document);
   const run=spawnSync(process.execPath,[resolve('scripts/release-bundle.mjs'),'--target','x86_64-unknown-linux-gnu'],{cwd,encoding:'utf8'});
   assert.notEqual(run.status,0);assert.match(run.stderr,/release-platforms.toml/);assert.match(run.stderr,/Next:/);
  }
  const stage=join(cwd,'package');mkdirSync(stage);
  const api=resolve('scripts/release-payload.mjs');
  writeFileSync(join(stage,'package.json'),'null');
  const invalidManifest=spawnSync(process.execPath,['--input-type=module','-e',`import {assertPackageDirectory} from ${JSON.stringify(api)}; assertPackageDirectory(${JSON.stringify(stage)},${JSON.stringify(stage)},'oneharness-cli','1.2.3');`],{encoding:'utf8'});
  assert.notEqual(invalidManifest.status,0);assert.match(invalidManifest.stderr,/Invalid package manifest/);
  for(const [returned,name,version] of [[join(cwd,'unexpected'),'oneharness-cli','1.2.3'],[stage,'wrong-package','1.2.3'],[stage,'oneharness-cli','9.9.9'],[join(cwd,'missing'),'oneharness-cli','1.2.3']]){
   writeFileSync(join(stage,'package.json'),JSON.stringify({name,version}));
   const expected=returned.endsWith('missing')?returned:stage;
   const script=`import {assertPackageDirectory} from ${JSON.stringify(api)}; assertPackageDirectory(${JSON.stringify(returned)},${JSON.stringify(expected)},'oneharness-cli','1.2.3');`;
   const run=spawnSync(process.execPath,['--input-type=module','-e',script],{encoding:'utf8'});
   assert.notEqual(run.status,0);assert.match(run.stderr,/Next:.*staging/);
  }
 }finally{rmSync(cwd,{recursive:true,force:true});}
});
// llmlint: ignore-end[shell_test_tiers_stay_split]
