// Drive the real Nx/just/Bun setup boundary in an owned checkout; installed Nx
// is a linked third-party dependency, while the private install starts absent.
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, copyFileSync, rmSync, symlinkSync, statSync, writeFileSync, utimesSync, readFileSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { execFileSync, spawnSync } from 'node:child_process';
import { parse } from 'yaml';
const work=mkdtempSync(join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR??tmpdir(),'workflow-setup-'));
try{
 for(const file of ['scripts/nx','justfile','package.json','bun.lock','nx.json','.github/package.json','.github/bun.lock','.github/project.json','.github/integration/project.json','tools/release-tooling/payload-contract/project.json','.github/variants-routing.mjs','.github/workflows/ci.yml','.github/workflows/e2e-variants.yml']){
  mkdirSync(dirname(join(work,file)),{recursive:true});copyFileSync(file,join(work,file));
  const source=statSync(file);utimesSync(join(work,file),source.atime,source.mtime);
 }
 const variants = parse(readFileSync('.github/workflows/e2e-variants.yml', 'utf8'));
 const install = variants.jobs.live.steps.find(step => step.name === 'Install workflow policy tooling');
 assert.ok(install, 'variants lane must install its policy tooling in a clean checkout');
 const args = ['.github/variants-routing.mjs', '--workflow', '.github/workflows/e2e-variants.yml', '--ci', '.github/workflows/ci.yml'];
 const packagePath = join(work, '.github/package.json');
 const packageText = readFileSync(packagePath, 'utf8');
 writeFileSync(packagePath, '{');
 const malformed = spawnSync('bash', ['-e', '-c', install.run], {cwd:work,encoding:'utf8'});
 assert.notEqual(malformed.status, 0);
 assert.match(malformed.stderr, /Workflow tooling install failed/);
 writeFileSync(packagePath, packageText);
 assert.equal(install.run, 'just github-install');
 execFileSync('bash', ['-e', '-c', install.run], {cwd:work,stdio:'inherit'});
 execFileSync(process.execPath,args,{cwd:work,stdio:'inherit'});
 rmSync(join(work,'.github/node_modules'),{recursive:true,force:true});
 mkdirSync(join(work,'node_modules'));symlinkSync(resolve('node_modules/nx'),join(work,'node_modules/nx'),'dir');
 copyFileSync('node_modules/.bun-install-stamp',join(work,'node_modules/.bun-install-stamp'));
 const completed=statSync('node_modules/.bun-install-stamp');utimesSync(join(work,'node_modules/.bun-install-stamp'),completed.atime,completed.mtime);
 const run=()=>JSON.parse(execFileSync('bash',['scripts/nx','show','projects','--json'],{cwd:work,encoding:'utf8',stdio:['ignore','pipe','pipe']}));
 assert.ok(run().includes('github-workflows'));
 const stamp=statSync(join(work,'.github/node_modules/.bun-install-stamp')).mtimeMs;
 assert.ok(run().includes('github-workflows-integration'));
 assert.equal(statSync(join(work,'.github/node_modules/.bun-install-stamp')).mtimeMs,stamp);
 let previous=stamp;
 for(const file of ['.github/bun.lock','.github/package.json']){
  const next=(previous+5)/1000;utimesSync(join(work,file),next,next);
  assert.ok(run().includes('github-workflows'));
  const refreshed=statSync(join(work,'.github/node_modules/.bun-install-stamp')).mtimeMs;
  assert.ok(refreshed>previous,`${file} freshness must reinstall the private package`);
  previous=refreshed;
 }
 console.log('workflow-setup: real Nx entry installs missing locked private tooling once in its own checkout');
}finally{rmSync(work,{recursive:true,force:true});}
