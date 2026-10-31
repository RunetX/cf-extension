import * as vscode from 'vscode';
import * as assert from 'node:assert/strict';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import type { ExtensionApi } from '../src/extension';
export async function run(): Promise<void> {
    const extension = vscode.extensions.getExtension<ExtensionApi>('RunetX.cf-extension');
    assert.ok(extension);
    if (process.env.CF_EXTENSION_PACKAGE_DIR) {
        assert.equal(vscode.workspace.getConfiguration('cfExtension').get('serverPath'), '', 'packaged smoke must use the bundled default server');
        assert.equal(path.resolve(extension.extensionPath).toLowerCase(), path.resolve(process.env.CF_EXTENSION_PACKAGE_DIR).toLowerCase());
    }
    const api = await extension.activate();
    try {
        const commands = await vscode.commands.getCommands(true);
        assert.ok(commands.includes('cfExtension.select'), 'native navigator select command is registered');
        const fixture = process.env.CF_EXTENSION_FIXTURE!;
        await vscode.commands.executeCommand('cfExtension.open', fixture);
        assert.equal(api.state().loaded, true);
        const roots = await api.tree();
        assert.ok(roots.some(n => n.label === 'NavigatorSynthetic'));
        const objects = await api.tree('group:Catalog');
        const catalog = objects.find(n => n.label === 'Products')!;
        assert.ok(catalog);
        await vscode.commands.executeCommand('cfExtension.select', catalog);
        const runtime = api as unknown as {
            properties(id: string): Promise<{
                id: string;
                properties: Array<{
                    name: string;
                    value: unknown;
                }>;
            }>;
            setProperty(id: string, name: string, value: unknown, revision: number): Promise<unknown>;
            search(q: string): Promise<Array<{
                id: string;
            }>>;
            references(id: string): Promise<unknown[]>;
            composition(id: string): Promise<unknown[]>;
            save(path?: string, format?: string): Promise<unknown>;
            close(discard?: boolean): Promise<unknown>;
        };
        const props = await runtime.properties(catalog.id);
        assert.ok(props.properties.some(p => p.name === 'Hierarchical'));
        await runtime.setProperty(catalog.id, 'Comment', 'Host saved comment', api.state().revision);
        assert.equal(api.state().dirty, true);
        await assert.rejects(runtime.close(), /несохран/i);
        await assert.rejects(runtime.setProperty(catalog.id, 'Comment', 'stale', 1), /изменилась/i);
        assert.ok((await runtime.search('PRODUCT')).some(n => n.id === catalog.id));
        assert.ok((await runtime.references(catalog.id)).length > 0);
        assert.equal((await runtime.composition(catalog.id)).length, 2);
        const save = path.join(path.dirname(fixture), 'host-roundtrip.cf');
        await vscode.commands.executeCommand('cfExtension.saveAs', { path: save, format: 'binary' });
        assert.equal(api.state().dirty, false);
        await runtime.close();
        await vscode.commands.executeCommand('cfExtension.open', save);
        assert.equal((await runtime.properties(catalog.id)).properties.find(p => p.name === 'Comment')?.value, 'Host saved comment');
        const xml = path.join(path.dirname(fixture), 'host-xml');
        await runtime.save(xml, 'xml');
        assert.ok((await fs.stat(path.join(xml, 'Configuration.xml'))).isFile());
        await assert.rejects(runtime.save(xml, 'xml'), /новый|пустой/i);
        await runtime.close();
        console.log('Real Extension Host: activation/open/tree/select/properties/edit/dirty/revision/search/references/composition/save/reopen/XML passed');
    }
    finally {
        api.dispose();
    }
}
