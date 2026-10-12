// Windows retains its existing builders and native install jobs, held to the
// platform contract by the fast tests. This journey drives the Unix builder.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
if (!['linux','darwin'].includes(process.platform)) {
 console.log('release integration: Unix bundle journey omitted on Windows; existing Windows release builders/install jobs retained');
} else {
 const target=`${process.arch==='arm64'?'aarch64':'x86_64'}-${process.platform==='darwin'?'apple-darwin':'unknown-linux-gnu'}`;
 const work=mkdtempSync(join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR??tmpdir(),'release-journey-'));
 const root=join(work,'bundle');
 try {
  for(const [command,args] of [
   ['just',['release-bundle',target,root]],
   [process.execPath,['scripts/release-install.mjs','--root',root]],
   [process.execPath,['.github/integration/transfer-journey.mjs','--root',root]],
   [process.execPath,['.github/integration/rejection-journey.mjs','--root',root]],
  ]) {
   const result=spawnSync(command,args,{stdio:'inherit'});
   if(result.error) throw result.error;
   if(result.status!==0){process.exitCode=result.status??1;break;}
  }
 } finally {rmSync(work,{recursive:true,force:true});}
}
