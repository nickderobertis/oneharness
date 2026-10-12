// Drive the real Nx/just/Bun setup boundary in an owned checkout; installed Nx
// is a linked third-party dependency, while the private install starts absent.
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, copyFileSync, rmSync, symlinkSync, statSync, writeFileSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
const work=mkdtempSync(join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR??tmpdir(),'workflow-setup-'));
try{
 for(const file of ['scripts/nx','justfile','package.json','bun.lock','nx.json','.github/package.json','.github/bun.lock','.github/project.json','.github/integration/project.json','tools/release-tooling/payload-contract/project.json']){
  mkdirSync(dirname(join(work,file)),{recursive:true});copyFileSync(file,join(work,file));
 }
 mkdirSync(join(work,'node_modules'));symlinkSync(resolve('node_modules/nx'),join(work,'node_modules/nx'),'dir');
 writeFileSync(join(work,'node_modules/.bun-install-stamp'),'');
 const run=()=>JSON.parse(execFileSync('bash',['scripts/nx','show','projects','--json'],{cwd:work,encoding:'utf8',stdio:['ignore','pipe','pipe']}));
 assert.ok(run().includes('github-workflows'));
 const stamp=statSync(join(work,'.github/node_modules/.bun-install-stamp')).mtimeMs;
 assert.ok(run().includes('github-workflows-integration'));
 assert.equal(statSync(join(work,'.github/node_modules/.bun-install-stamp')).mtimeMs,stamp);
 console.log('workflow-setup: real Nx entry installs missing locked private tooling once in its own checkout');
}finally{rmSync(work,{recursive:true,force:true});}
