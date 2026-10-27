'use strict';
const assert=require('assert'),fs=require('fs'),os=require('os'),path=require('path'),crypto=require('crypto');
const {identity,protectedFile,writeReceipt}=require('./write-local-cache-receipt.cjs');
const meta={dev:7n,ino:9007199254740993n,size:6n,mtimeNs:1793087207043977101n,ctimeNs:1793087208939977099n,uid:0n,mode:0o100444n,isFile:()=>true};
assert.strictEqual(identity(meta).inode,'9007199254740993');
assert.strictEqual(identity(meta).modifiedNs,'1793087207043977101');
assert(protectedFile(meta));
assert(!protectedFile({...meta,uid:501n}));
assert(!protectedFile({...meta,mode:0o100644n}));
assert(!protectedFile({...meta,isFile:()=>false}));
if(process.getuid()===0){
 const root=fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(),'local-receipt-')));
 fs.chmodSync(root,0o755);
 const bytes=Buffer.from('abcdef'),sha=crypto.createHash('sha256').update(bytes).digest('hex'),media=path.join(root,sha+'.mp4');
 try{
  fs.writeFileSync(media,bytes,{mode:0o444});
  const oldMask=process.umask(0o077);let result;
  try{result=writeReceipt(root,sha,bytes.length);}finally{process.umask(oldMask);}
  const receipt=JSON.parse(fs.readFileSync(result));
  assert.strictEqual(receipt.sha256,sha);assert.deepStrictEqual(receipt.file,identity(fs.statSync(media,{bigint:true})));
  assert.strictEqual(fs.statSync(result).mode&0o777,0o444);
  fs.chmodSync(root,0o775);assert.throws(()=>writeReceipt(root,sha,bytes.length),/Unprotected/);fs.chmodSync(root,0o755);
  fs.chmodSync(media,0o644);assert.throws(()=>writeReceipt(root,sha,bytes.length),/read-only/);
  fs.writeFileSync(media,'ABCDEF');fs.chmodSync(media,0o444);assert.throws(()=>writeReceipt(root,sha,bytes.length),/hash or identity changed/);
  fs.renameSync(media,media+'.actual');fs.symlinkSync(media+'.actual',media);assert.throws(()=>writeReceipt(root,sha,bytes.length));
  assert.deepStrictEqual(fs.readdirSync(root).filter(n=>n.startsWith('.verified-')),[]);
 }finally{fs.rmSync(root,{recursive:true,force:true});}
}else{
 assert.throws(()=>writeReceipt('/unused','a'.repeat(64),6),/Administrative root required/);
}
console.log('local cache receipt writer: identity, ownership, atomic hash and rejection checks passed');
