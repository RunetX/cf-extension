import { downloadAndUnzipVSCode } from '@vscode/test-electron';
import * as fs from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
async function main(): Promise<void> {
    const root = path.resolve(__dirname,'../../..');
    const server = process.env.CF_EXTENSION_SERVER || path.join(root,'target','debug',process.platform === 'win32' ? 'model-server.exe' : 'model-server');
    const temp = await fs.mkdtemp(path.join(os.tmpdir(),'cf-extension-host-'));
    try {
        const workspace = path.join(temp,'workspace'); const profile = path.join(temp,'profile');
        await fs.mkdir(workspace,{recursive:true}); await fs.mkdir(path.join(profile,'User'),{recursive:true});
        await fs.writeFile(path.join(profile,'User','settings.json'),JSON.stringify({'cfExtension.serverPath':server}));
        const fixture=path.join(workspace,'synthetic.cf');
        const generated=spawnSync(server,['--generate-fixture',fixture],{encoding:'utf8',windowsHide:true});
        if (generated.error || generated.status !== 0) { throw generated.error || new Error(generated.stderr); }
        const executable = await downloadAndUnzipVSCode(process.env.VSCODE_TEST_VERSION || '1.95.3');
        const environment: NodeJS.ProcessEnv = { ...process.env, CF_EXTENSION_FIXTURE:fixture };
        delete environment.ELECTRON_RUN_AS_NODE;
        const args = [workspace, '--extensionDevelopmentPath='+path.resolve(root,'extension'), '--extensionTestsPath='+path.resolve(__dirname,'host-smoke.js'), '--user-data-dir',profile,'--extensions-dir',path.join(temp,'extensions'),'--disable-workspace-trust','--skip-welcome','--skip-release-notes','--disable-extensions'];
        await new Promise<void>((resolve,reject)=>{
            const host=spawn(executable,args,{env:environment,windowsHide:true,shell:false,stdio:'inherit'});
            host.on('error',reject);
            host.on('exit',(code,signal)=>code===0 ? resolve() : reject(new Error(`Extension Host failed: ${code}, ${signal}`)));
        });
    } finally { await fs.rm(temp,{recursive:true,force:true}); }
}
void main().catch(error=>{ console.error(error);process.exitCode=1; });
