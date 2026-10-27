#!/usr/bin/env node
'use strict';
// Administrative operation only. The application never creates its own trust receipts.
const fs=require('fs'), crypto=require('crypto'), path=require('path');
const ROOT='/media/internal/pinpin-family/cache', MAX=1024*1024*1024;
function identity(s){return {device:String(s.dev),inode:String(s.ino),size:String(s.size),modifiedNs:String(s.mtimeNs),changedNs:String(s.ctimeNs)};}
function protectedFile(s){return s.isFile()&&s.uid===0n&&(s.mode&0o222n)===0n;}
function writeReceipt(root,sha256,bytes){
 if(process.getuid()!==0)throw Error('Administrative root required');
 if(!/^[0-9a-f]{64}$/.test(sha256)||!Number.isSafeInteger(bytes)||bytes<=0||bytes>MAX)throw Error('Invalid content specification');
 const dir=fs.lstatSync(root,{bigint:true});
 if(fs.realpathSync(root)!==root||!dir.isDirectory()||dir.uid!==0n||(dir.mode&0o022n)!==0n)throw Error('Unprotected cache directory');
 const flags=fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW|fs.constants.O_NONBLOCK;
 if(!fs.constants.O_NOFOLLOW)throw Error('NOFOLLOW required');
 const media=path.join(root,sha256+'.mp4'),fd=fs.openSync(media,flags);let temp;
 try {
  const before=fs.fstatSync(fd,{bigint:true});
  if(!protectedFile(before)||before.size!==BigInt(bytes)||typeof before.mtimeNs!=='bigint'||typeof before.ctimeNs!=='bigint')throw Error('Media must be root-owned, read-only and have exact nanosecond metadata');
  const hash=crypto.createHash('sha256'),buffer=Buffer.alloc(65536);let count=0,n;
  while((n=fs.readSync(fd,buffer,0,buffer.length,null))>0){count+=n;if(count>bytes)throw Error('Media grew');hash.update(buffer.subarray(0,n));}
  const after=fs.fstatSync(fd,{bigint:true}),named=fs.lstatSync(media,{bigint:true});
  if(count!==bytes||hash.digest('hex')!==sha256||!protectedFile(after)||!protectedFile(named)||JSON.stringify(identity(before))!==JSON.stringify(identity(after))||JSON.stringify(identity(after))!==JSON.stringify(identity(named)))throw Error('Media hash or identity changed');
  const receipt={version:1,sha256,bytes,file:identity(after)};
  temp=path.join(root,'.verified-'+process.pid+'-'+crypto.randomBytes(8).toString('hex'));
  const output=fs.openSync(temp,fs.constants.O_WRONLY|fs.constants.O_CREAT|fs.constants.O_EXCL|fs.constants.O_NOFOLLOW,0o444);
  try{fs.writeFileSync(output,JSON.stringify(receipt)+'\n');fs.fchmodSync(output,0o444);fs.fsyncSync(output);}finally{fs.closeSync(output);}
  const destination=path.join(root,sha256+'.verified.json');fs.renameSync(temp,destination);temp=undefined;
  const directory=fs.openSync(root,fs.constants.O_RDONLY|fs.constants.O_DIRECTORY);try{fs.fsyncSync(directory);}finally{fs.closeSync(directory);}
  return destination;
 }finally{fs.closeSync(fd);if(temp)try{fs.unlinkSync(temp);}catch(_){}}
}
if(require.main===module){try{if(process.argv.length!==4)throw Error('Usage: write-local-cache-receipt.cjs SHA256 BYTES');console.log(writeReceipt(ROOT,process.argv[2],Number(process.argv[3])));}catch(error){console.error(error.message);process.exitCode=1;}}
module.exports={identity,protectedFile,writeReceipt};
