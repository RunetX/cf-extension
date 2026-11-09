import { downloadAndUnzipVSCode } from '@vscode/test-electron';
import * as fs from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
async function main(): Promise<void> {
    const root = path.resolve(__dirname, '../../..');
    const server = process.env.CF_EXTENSION_SERVER || path.join(root, 'target', 'debug', process.platform === 'win32' ? 'model-server.exe' : 'model-server');
    const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-extension-host-'));
    try {
        const workspace = path.join(temp, 'workspace');
        const profile = path.join(temp, 'profile');
        await fs.mkdir(workspace, { recursive: true });
        await fs.mkdir(path.join(profile, 'User'), { recursive: true });
        if (!process.env.CF_EXTENSION_PACKAGE_DIR) {
            await fs.writeFile(path.join(profile, 'User', 'settings.json'), JSON.stringify({ 'cfExtension.serverPath': server }));
        }
        const fixture = path.join(workspace, 'synthetic.cf');
        const generated = spawnSync(server, ['--generate-fixture', fixture], { encoding: 'utf8', windowsHide: true });
        if (generated.error || generated.status !== 0) {
            throw generated.error || new Error(generated.stderr);
        }
        const executable = process.env.VSCODE_EXECUTABLE_PATH || await downloadAndUnzipVSCode(process.env.VSCODE_TEST_VERSION || '1.95.3');
        const environment: NodeJS.ProcessEnv = { ...process.env, CF_EXTENSION_FIXTURE: fixture };
        delete environment.ELECTRON_RUN_AS_NODE;
        const args = [workspace, '--extensionDevelopmentPath=' + (process.env.CF_EXTENSION_PACKAGE_DIR || path.resolve(root, 'extension')), '--extensionTestsPath=' + path.resolve(__dirname, 'host-smoke.js'), '--user-data-dir', profile, '--extensions-dir', path.join(temp, 'extensions'), '--disable-workspace-trust', '--disable-crash-reporter', '--skip-welcome', '--skip-release-notes', '--disable-extensions'];
        // GitHub's isolated Linux runner cannot use the downloaded Electron SUID helper.
        // This affects only the test Host, never the installed extension or normal local runs.
        if (process.platform === 'linux' && process.env.GITHUB_ACTIONS === 'true') {
            args.push('--no-sandbox');
        }
        await new Promise<void>((resolve, reject) => {
            const host = spawn(executable, args, { env: environment, windowsHide: true, shell: false, stdio: 'inherit' });
            const timeout = setTimeout(() => { host.kill(); reject(new Error('Extension Host timeout (120 seconds)')); }, 120000);
            host.on('error', error => { clearTimeout(timeout); reject(error); });
            host.on('exit', (code, signal) => { clearTimeout(timeout); if (code === 0) {
                resolve();
            }
            else {
                reject(new Error('Extension Host failed: ' + code + ', ' + signal));
            } });
        });
    }
    finally {
        await fs.rm(temp, { recursive: true, force: true, maxRetries: 30, retryDelay: 200 });
    }
}
void main().catch(error => { console.error(error); process.exitCode = 1; });
