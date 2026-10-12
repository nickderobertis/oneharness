// Drive the real Nx entry point from a checkout lacking its private tooling install.
import assert from 'node:assert/strict';
import { mkdtempSync, renameSync, rmSync, existsSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
const work=mkdtempSync(join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR??tmpdir(),'workflow-setup-'));
const modules='.github/node_modules', backup=join(work,'original-modules');
const owned=existsSync(modules);
try{
 if(owned)renameSync(modules,backup);
 const run=()=>JSON.parse(execFileSync('bash',['scripts/nx','show','projects','--json'],{encoding:'utf8',stdio:['ignore','pipe','pipe']}));
 assert.ok(run().includes('github-workflows'));
 const stamp=statSync(join(modules,'.bun-install-stamp')).mtimeMs;
 assert.ok(run().includes('github-workflows-integration'));
 assert.equal(statSync(join(modules,'.bun-install-stamp')).mtimeMs,stamp);
 console.log('workflow-setup: real Nx entry installs missing locked private tooling once');
}finally{
 rmSync(modules,{recursive:true,force:true});
 if(owned && existsSync(backup))renameSync(backup,modules);
 rmSync(work,{recursive:true,force:true});
}
