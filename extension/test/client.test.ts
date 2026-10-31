import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import { RpcClient } from '../src/client';
test('framed client handles fragmented UTF-8 response and rejects process exit', async () => {
    const script = `let data=Buffer.alloc(0);process.stdin.on('data',chunk=>{data=Buffer.concat([data,chunk]);let end=data.indexOf('\\r\\n\\r\\n');if(end<0)return;let n=Number(data.subarray(0,end).toString().split(':')[1]);if(data.length<end+4+n)return;let r=JSON.parse(data.subarray(end+4,end+4+n));data=data.subarray(end+4+n);if(r.method==='exit'){process.exit(0);}else{let body=Buffer.from(JSON.stringify({jsonrpc:'2.0',id:r.id,result:{label:'Синтетика'}}));let out=Buffer.concat([Buffer.from('Content-Length: '+body.length+'\\r\\n\\r\\n'),body]);let at=0;function send(){if(at<out.length){process.stdout.write(out.subarray(at,++at));setImmediate(send);}}send();}});`;
    const client = new RpcClient(process.execPath, ['-e',script]);
    try {
        assert.deepEqual(await client.request('initialize'), { label: 'Синтетика' });
        await assert.rejects(client.request('exit'), /заверш|exit/i);
    } finally { client.dispose(); }
});
