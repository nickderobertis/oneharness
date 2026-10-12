// Exercise the real installer with defective staged products, never its internals.
import assert from 'node:assert/strict';
import { cpSync, mkdtempSync, readFileSync, writeFileSync, rmSync, readdirSync, renameSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { parseArgs } from 'node:util';
const {values} = parseArgs({options:{root:{type:'string'}}});
const root=resolve(values.root), target=`${process.arch==='arm64'?'aarch64':'x86_64'}-${process.platform==='darwin'?'apple-darwin':'unknown-linux-gnu'}`;
const work=mkdtempSync(join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR??tmpdir(),'release-rejections-'));
let count=0;
function reject(label, mutate, pattern){
 const stage=join(work,`${count++}`);cpSync(join(root,target),join(stage,target),{recursive:true});
 const dir=join(stage,target), record=JSON.parse(readFileSync(join(dir,'payload.json'),'utf8'));
 mutate(dir,record);
 const run=spawnSync(process.execPath,['scripts/release-install.mjs','--root',stage],{encoding:'utf8'});
 assert.notEqual(run.status,0,`${label} unexpectedly installed`);assert.match(run.stderr,pattern,label);
}
function rewriteArchive(dir,record,content){
 const archive=join(dir,`oneharness-${record.tag}-${target}.tar.gz`);
 execFileSync('python3',['-c',`import tarfile,io,sys
archive,kind,content=sys.argv[1:]
with tarfile.open(archive,'w:gz') as tar:
 for name in ['oneharness','README.md','LICENSE','CHANGELOG.md']:
  data=content.encode() if name=='oneharness' else b'document'
  item=tarfile.TarInfo(name);item.size=len(data);item.mode=0o755;tar.addfile(item,io.BytesIO(data))
 if kind=='unsafe':
  item=tarfile.TarInfo('../escaped');item.size=1;tar.addfile(item,io.BytesIO(b'x'))
`,archive,content==='unsafe'?'unsafe':'regular',content]);
}
try{
 reject('malformed metadata',(dir)=>writeFileSync(join(dir,'payload.json'),'{'),/Release installation proof failed/);
 reject('invalid metadata schema',(dir,r)=>{r.schema_version=999;writeFileSync(join(dir,'payload.json'),JSON.stringify(r));},/Invalid release payload metadata/);
 reject('archive link',(dir,r)=>{
  const archive=join(dir,`oneharness-${r.tag}-${target}.tar.gz`);
  execFileSync('python3',['-c',`import tarfile,io,sys
with tarfile.open(sys.argv[1],'w:gz') as t:
 for name in ['oneharness','README.md','LICENSE','CHANGELOG.md']:
  item=tarfile.TarInfo(name)
  if name=='oneharness': item.type=tarfile.SYMTYPE;item.linkname='README.md';t.addfile(item)
  else: item.size=1;t.addfile(item,io.BytesIO(b'x'))
`,archive]);
 },/Unsafe archive members/);
 reject('wrong executable CPU',(dir,r)=>{
  const binary=join(dir,'archive','oneharness'), bytes=readFileSync(binary);
  if(process.platform==='linux') bytes.writeUInt16LE(process.arch==='arm64'?62:183,18);
  else bytes.writeUInt32LE(process.arch==='arm64'?0x01000007:0x0100000c,4);
  writeFileSync(binary,bytes);
  execFileSync('tar',['-czf',join(dir,`oneharness-${r.tag}-${target}.tar.gz`),'-C',join(dir,'archive'),'oneharness','README.md','LICENSE','CHANGELOG.md']);
 },/Artifact architecture mismatch/);
 reject('target metadata' ,(dir,r)=>{r.target=target==='aarch64-apple-darwin'?'x86_64-apple-darwin':'aarch64-apple-darwin';writeFileSync(join(dir,'payload.json'),JSON.stringify(r));},/Staged target/);
 for(const mode of ['missing','duplicate','renamed']) reject(`archive ${mode}`,(dir)=>{const a=readdirSync(dir).find(f=>f.endsWith('.tar.gz'));if(mode==='missing')rmSync(join(dir,a));else if(mode==='duplicate')cpSync(join(dir,a),join(dir,'duplicate.tar.gz'));else renameSync(join(dir,a),join(dir,'renamed.tar.gz'));},/Expected .*tar.gz/);
 reject('unsafe archive',(dir,r)=>rewriteArchive(dir,r,'unsafe'),/Unsafe archive members/);
 reject('architecture metadata',(dir,r)=>{r.wheel=r.wheel.replace(/_(?:x86_64|arm64|aarch64)\.whl$/,process.arch==='arm64'?'_x86_64.whl':'_arm64.whl');writeFileSync(join(dir,'payload.json'),JSON.stringify(r));},/Artifact architecture mismatch/);
 reject('corrupt bytes',(dir,r)=>{r.binarySha256='0'.repeat(64);writeFileSync(join(dir,'payload.json'),JSON.stringify(r));},/Payload reuse failed/);
 reject('CLI version',(dir,r)=>{r.version='99.99.99';r.tag='v99.99.99';const old=readdirSync(dir).find(f=>f.endsWith('.tar.gz'));renameSync(join(dir,old),join(dir,`oneharness-${r.tag}-${target}.tar.gz`));writeFileSync(join(dir,'payload.json'),JSON.stringify(r));},/Unexpected CLI version/);
 // A staged executable may be wrong despite matching its declared hash and CPU.
 // Patch the serialized status string to prove planned-report rejection.
 reject('planned report',(dir,r)=>{
  const binary=join(dir,'archive','oneharness');let bytes=readFileSync(binary);
  const original=Buffer.from('planned'), replacement=Buffer.from('broken!');let at=bytes.indexOf(original);assert.ok(at>=0);while(at>=0){replacement.copy(bytes,at);at=bytes.indexOf(original,at+7);}writeFileSync(binary,bytes);
  r.binarySha256=createHash('sha256').update(bytes).digest('hex');writeFileSync(join(dir,'payload.json'),JSON.stringify(r));
  execFileSync('tar',['-czf',join(dir,`oneharness-${r.tag}-${target}.tar.gz`),'-C',join(dir,'archive'),'oneharness','README.md','LICENSE','CHANGELOG.md']);
 },/Missing planned command contract/);
 // llmlint: ignore-block[e2e_not_mocked] This staged executable is a deliberately defective product input; the installer under test runs for real, while the happy path uses the real compiled CLI.
 reject('null CLI report',(dir,r)=>{
  const binary=join(dir,'archive','oneharness'), source=join(work,'null-report.rs');
  writeFileSync(source,`fn main(){ let a:Vec<String>=std::env::args().collect(); if a.contains(&"--version".into()){println!("oneharness ${r.version}");}else if a.contains(&"--print-command".into()){println!("null");}else{std::process::exit(2);} }`);
  execFileSync('rustc',[source,'-o',binary]);
  r.binarySha256=createHash('sha256').update(readFileSync(binary)).digest('hex');writeFileSync(join(dir,'payload.json'),JSON.stringify(r));
  execFileSync('tar',['-czf',join(dir,`oneharness-${r.tag}-${target}.tar.gz`),'-C',join(dir,'archive'),'oneharness','README.md','LICENSE','CHANGELOG.md']);
 },/Missing planned command contract/);
 // llmlint: ignore-end[e2e_not_mocked]
 // llmlint: ignore-block[e2e_not_mocked] This staged executable is a deliberately defective product input; the installer under test runs for real, while the happy path uses the real compiled CLI.
 reject('usage executable disappears',(dir,r)=>{
  const binary=join(dir,'archive','oneharness'), source=join(work,'disappearing.rs');
  writeFileSync(source,`fn main(){ let a:Vec<String>=std::env::args().collect(); if a.contains(&"--version".into()){println!("oneharness ${r.version}");}else if a.contains(&"--print-command".into()){println!("{}",r#"{"results":[{"status":"planned","command":["hello"]}]}"#);std::fs::remove_file(std::env::current_exe().unwrap()).unwrap();}else{std::process::exit(2);} }`);
  execFileSync('rustc',[source,'-o',binary]);
  r.binarySha256=createHash('sha256').update(readFileSync(binary)).digest('hex');writeFileSync(join(dir,'payload.json'),JSON.stringify(r));
  execFileSync('tar',['-czf',join(dir,`oneharness-${r.tag}-${target}.tar.gz`),'-C',join(dir,'archive'),'oneharness','README.md','LICENSE','CHANGELOG.md']);
 },/Cannot start usage proof: .*ENOENT/);
 // llmlint: ignore-end[e2e_not_mocked]
 // llmlint: ignore-block[e2e_not_mocked] This staged executable is a deliberately defective product input; the installer under test runs for real, while the happy path uses the real compiled CLI.
 reject('usage exit',(dir,r)=>{
  const binary=join(dir,'archive','oneharness'), source=join(work,'defective.rs');
  writeFileSync(source,`fn main(){ let a:Vec<String>=std::env::args().collect(); if a.contains(&"--version".into()){println!("oneharness ${r.version}");}else if a.contains(&"--print-command".into()){println!("{{\\\"results\\\":[{{\\\"status\\\":\\\"planned\\\",\\\"command\\\":[\\\"hello\\\"]}}]}}");} }`);
  execFileSync('rustc',[source,'-o',binary]);
  r.binarySha256=createHash('sha256').update(readFileSync(binary)).digest('hex');writeFileSync(join(dir,'payload.json'),JSON.stringify(r));
  execFileSync('tar',['-czf',join(dir,`oneharness-${r.tag}-${target}.tar.gz`),'-C',join(dir,'archive'),'oneharness','README.md','LICENSE','CHANGELOG.md']);
 },/Invalid harness exited 0/);
 // llmlint: ignore-end[e2e_not_mocked]
 reject('wheel payload',(dir,r)=>{
  execFileSync('python3',['-c',`import zipfile,sys,os
path=sys.argv[1]
with zipfile.ZipFile(path) as z: entries=[(i,z.read(i.filename)) for i in z.infolist()]
with zipfile.ZipFile(path+'.new','w') as z:
 for i,data in entries: z.writestr(i,data+b'corrupt' if i.filename.endswith('.data/scripts/oneharness') else data)
os.replace(path+'.new',path)
`,join(dir,'wheels',r.wheel)]);
 },/Payload reuse failed/);
 reject('npm payload',(dir)=>{
  const tarball=join(dir,'npm',readdirSync(join(dir,'npm')).find(f=>f.endsWith('.tgz')));
  execFileSync('python3',['-c',`import tarfile,io,sys,os
path=sys.argv[1]
with tarfile.open(path) as t: entries=[(i,t.extractfile(i).read() if i.isfile() else None) for i in t.getmembers()]
with tarfile.open(path+'.new','w:gz') as t:
 for i,data in entries:
  if i.name=='package/bin/oneharness': data+=b'corrupt';i.size=len(data)
  t.addfile(i,io.BytesIO(data) if data is not None else None)
os.replace(path+'.new',path)
`,tarball]);
 },/Payload reuse failed/);
 const payload=JSON.parse(readFileSync(join(root,target,'payload.json'),'utf8'));
 const proof=JSON.parse(readFileSync(join(root,target,'native-install.json'),'utf8'));
 assert.equal(proof.target,target);assert.equal(proof.version,payload.version);assert.equal(proof.platform,process.platform);assert.equal(proof.architecture,process.arch);
 assert.match(proof.binaryDescription,process.arch==='arm64'?/arm64|aarch64/:/x86[_-]64/);
 assert.deepEqual(proof.payloads,{archive:payload.binarySha256,wheel:payload.binarySha256,npm:payload.binarySha256});
 for(const mode of ['missing','duplicate','renamed']) reject(`npm ${mode}`,(dir)=>{const npm=join(dir,'npm'),a=readdirSync(npm).find(f=>f.endsWith('.tgz'));if(mode==='missing')rmSync(join(npm,a));else if(mode==='duplicate')cpSync(join(npm,a),join(npm,'duplicate.tgz'));else renameSync(join(npm,a),join(npm,'renamed.tgz'));},/Expected .*tgz/);
 const mismatchedTransfer=join(work,'mismatched-transfer');cpSync(join(root,target),join(mismatchedTransfer,target),{recursive:true});
 const mismatchPath=join(mismatchedTransfer,target,'payload.json'), mismatch=JSON.parse(readFileSync(mismatchPath,'utf8'));
 mismatch.target=target==='aarch64-apple-darwin'?'x86_64-apple-darwin':'aarch64-apple-darwin';writeFileSync(mismatchPath,JSON.stringify(mismatch));
 const mismatchRun=spawnSync(process.execPath,['.github/integration/transfer-journey.mjs','--root',mismatchedTransfer],{encoding:'utf8'});
 assert.notEqual(mismatchRun.status,0);assert.match(mismatchRun.stderr,/Transfer target .* differs from metadata/);
 const invalidDirectory=join(work,'invalid-directory');cpSync(join(root,target),join(invalidDirectory,target+';true'),{recursive:true});
 const invalidDirectoryRun=spawnSync(process.execPath,['.github/integration/transfer-journey.mjs','--root',invalidDirectory],{encoding:'utf8'});
 assert.notEqual(invalidDirectoryRun.status,0);assert.match(invalidDirectoryRun.stderr,/Missing supported native target/);
 const invalidChecksum=join(work,'invalid-checksum');cpSync(join(root,target),join(invalidChecksum,target),{recursive:true});
 const checksum=readdirSync(join(invalidChecksum,target)).find(f=>f.endsWith('.sha256'));
 writeFileSync(join(invalidChecksum,target,checksum),'0'.repeat(64)+'  wrong-archive.tar.gz\n');
 const transfer=spawnSync(process.execPath,['.github/integration/transfer-journey.mjs','--root',invalidChecksum],{encoding:'utf8'});
 assert.notEqual(transfer.status,0);assert.match(transfer.stderr,/Incorrect archive checksum/);
 console.log(`release-rejections: ${count} defective staged products rejected by the real installer`);
}finally{rmSync(work,{recursive:true,force:true});}
