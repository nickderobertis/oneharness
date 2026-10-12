// The consolidated builder targets Unix; Windows retains its existing builders
// and native install jobs, held to the platform contract by the fast tests.
import { spawnSync } from 'node:child_process';
if (!['linux','darwin'].includes(process.platform)) {
 console.log('release integration: Unix bundle journey omitted on Windows; existing Windows release builders/install jobs retained');
} else {
 const target=`${process.arch==='arm64'?'aarch64':'x86_64'}-${process.platform==='darwin'?'apple-darwin':'unknown-linux-gnu'}`;
 for(const [command,args] of [
  ['just',['release-bundle',target]],
  [process.execPath,['scripts/release-install.mjs','--root','dist/release']],
  [process.execPath,['.github/integration/transfer-journey.mjs','--root','dist/release']],
  [process.execPath,['.github/integration/rejection-journey.mjs','--root','dist/release']],
  [process.execPath,['.github/run-budgets.mjs']],
 ]) {
  const result=spawnSync(command,args,{stdio:'inherit'});
  if(result.error) throw result.error;
  if(result.status!==0){process.exitCode=result.status??1;break;}
 }
}
