import * as vscode from 'vscode';
import * as assert from 'node:assert/strict';
import type { ExtensionApi } from '../src/extension';
export async function run(): Promise<void> {
    const extension=vscode.extensions.getExtension<ExtensionApi>('RunetX.cf-extension');
    assert.ok(extension,'Extension must be installed in the real Extension Host');
    const api=await extension.activate();
    try {
        await vscode.commands.executeCommand('cfExtension.open',process.env.CF_EXTENSION_FIXTURE);
        assert.equal(api.state().loaded,true);
        assert.ok((await api.tree()).some(node=>node.label === 'NavigatorSynthetic'));
        console.log('Real Extension Host scaffold: activation/open/tree passed');
    } finally { api.dispose(); }
}
