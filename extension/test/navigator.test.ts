import {test} from 'node:test';
import * as assert from 'node:assert/strict';
// Load the public provider with a minimal VS Code host (no private fields inspected).
const Module = require('node:module');
const load = Module._load;
Module._load = function(name: string, ...args: unknown[]) { if(name === 'vscode') return {}; return load.call(this,name,...args); };
const {PropertyPalette} = require('../src/navigator') as typeof import('../src/navigator');
Module._load = load;
test('rejected edit A followed by select B eventually renders B',async()=>{
    let reject!: (reason:Error)=>void;
    let receive!: (m:unknown)=>void;
    const messages: any[]=[];
    const api = {state:()=>({loaded:true,revision:1,sessionId:1,dirty:false,sourcePath:'x'}),properties:async(id:string)=>({id,properties:[{name:'Comment',label:'Комментарий',kind:'string' as const,value:id,editable:true}]}),setProperty:()=>new Promise<any>((_,r)=>{reject=r;}),tree:async()=>[],node:async()=>({id:'',label:'',kind:'',hasChildren:false})};
    const palette=new PropertyPalette(api);
    palette.resolveWebviewView({webview:{options:{},html:'',postMessage:(m:unknown)=>{messages.push(m);return Promise.resolve(true);},onDidReceiveMessage:(f:typeof receive)=>{receive=f;}},onDidDispose:()=>{}} as any);
    await palette.select('A');
    receive({type:'edit',id:'A',revision:1,name:'Comment',value:'new'});
    await palette.select('B');
    reject(new Error('controlled rejection A'));
    await new Promise(r=>setImmediate(r));
    assert.equal(messages.filter(m=>m.type==='properties').at(-1)?.id,'B');
});
