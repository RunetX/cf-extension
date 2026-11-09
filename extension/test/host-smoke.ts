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
    const openCommand = extension.packageJSON.contributes.commands.find((c: { command: string }) => c.command === 'cfExtension.open');
    assert.equal(openCommand?.title, 'Конструктор: Открыть конфигурацию', 'open command has a searchable Russian title');
    const fixture = process.env.CF_EXTENSION_FIXTURE!;
    // Exercise command activation, not an explicit extension.activate() shortcut.
    await vscode.commands.executeCommand('cfExtension.open', fixture);
    assert.ok(extension.isActive, 'open command activates the extension');
    const api = extension.exports;
    try {
        const commands = await vscode.commands.getCommands(true);
        assert.ok(commands.includes('cfExtension.select'), 'native navigator select command is registered');
        assert.equal(api.state().loaded, true);
        const roots = await api.tree();
        assert.ok(roots.some(n => n.label === 'NavigatorSynthetic'));
        assert.equal(roots.length, 1, 'configuration is the sole root');
        const rootChildren = await api.tree(roots[0].id);
        assert.equal(rootChildren[0].label, 'Общие');
        assert.equal(rootChildren[0].parentId, roots[0].id);
        assert.ok((await api.tree('common')).some(n => n.label === 'Общие модули'));
        assert.equal((await api.node('group:Catalog')).parentId, roots[0].id);
        const objects = await api.tree('group:Catalog');
        const catalog = objects.find(n => n.label === 'Products')!;
        assert.ok(catalog);
        const collections = await api.tree(catalog.id);
        assert.deepEqual(collections.filter(n => n.kind !== 'module').map(n => n.label), ['Реквизиты', 'Табличные части', 'Формы', 'Команды', 'Макеты']);
        const attributes = await api.tree(collections[0].id);
        assert.equal(attributes[0].parentId, collections[0].id);
        await vscode.commands.executeCommand('cfExtension.select', attributes[0]);
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
        const moduleInfos = await api.modules(catalog.id);
        assert.ok(moduleInfos.length >= 2, 'existing object and manager modules');
        const doc = await api.openModule(moduleInfos[0].id);
        assert.equal(doc.languageId, 'bsl');
        assert.equal(doc.uri.scheme, 'cf-module');
        assert.ok(doc.uri.path.endsWith('.bsl'));
        assert.equal((await api.openModule(moduleInfos[0].id)).uri.toString(), doc.uri.toString());
        const originalFile = await fs.readFile(fixture);
        const text = '// Host module edit 😀\nПроцедура Несохраненный(\n';
        const edit = new vscode.WorkspaceEdit();
        edit.replace(doc.uri, new vscode.Range(doc.positionAt(0),doc.positionAt(doc.getText().length)),text);
        assert.ok(await vscode.workspace.applyEdit(edit));
        assert.equal(doc.isDirty,true);
        assert.ok(await doc.save());
        assert.equal(doc.isDirty,false);
        assert.equal(api.state().dirty,true);
        assert.deepEqual(await fs.readFile(fixture),originalFile,'Ctrl+S changes only the model');
        const waitFor = async (what: () => boolean, label: string): Promise<void> => {
            const deadline = Date.now() + 15000;
            while (!what()) { if (Date.now() > deadline) throw new Error(`timeout waiting for ${label}`); await new Promise(r => setTimeout(r, 100)); }
        };
        // Broken text is already in the buffer: diagnostics appear live, without Ctrl+S.
        await waitFor(() => vscode.languages.getDiagnostics(doc.uri).length > 0, 'BSL diagnostics');
        assert.ok(vscode.languages.getDiagnostics(doc.uri).some(d => d.source === 'Конструктор BSL'), 'BSL diagnostic source');
        const fix = new vscode.WorkspaceEdit();
        fix.replace(doc.uri, new vscode.Range(doc.positionAt(0), doc.positionAt(doc.getText().length)), '// Host module edit 😀\nПроцедура Несохраненный()\nКонецПроцедуры\n');
        assert.ok(await vscode.workspace.applyEdit(fix));
        await waitFor(() => vscode.languages.getDiagnostics(doc.uri).length === 0, 'diagnostics cleared');
        // Ctrl+Space и F12 без Ctrl+S: подсказки по метаданным, межмодульные переходы.
        const cmName = (await api.tree('group:CommonModule'))[0].label;
        const navText = [
            '// Host navigation',
            'Процедура П()',
            '\tСправочники.',
            '\tСправочники.Products.ПустаяСсылка()',
            '\tСправочники.Products.Example()',
            `\t${cmName}.Example()`,
            'КонецПроцедуры',
        ].join('\n');
        const navEdit = new vscode.WorkspaceEdit();
        navEdit.replace(doc.uri, new vscode.Range(doc.positionAt(0), doc.positionAt(doc.getText().length)), navText);
        assert.ok(await vscode.workspace.applyEdit(navEdit));
        const completions = await vscode.commands.executeCommand<vscode.CompletionList>('vscode.executeCompletionItemProvider', doc.uri, new vscode.Position(2, 13));
        assert.ok(completions && completions.items.some(i => i.label === 'Products'), 'Ctrl+Space: Справочники. → Products');
        const metaDefs = await vscode.commands.executeCommand<vscode.Location[]>('vscode.executeDefinitionProvider', doc.uri, new vscode.Position(3, 15));
        assert.ok(metaDefs && metaDefs.length === 0, 'F12 к метаданным раскрывает объект дерева, файл не открывается');
        const managerDefs = await vscode.commands.executeCommand<vscode.Location[]>('vscode.executeDefinitionProvider', doc.uri, new vscode.Position(4, 24));
        assert.ok(managerDefs?.length === 1 && managerDefs[0].uri.scheme === 'cf-module' && managerDefs[0].uri.toString() !== doc.uri.toString(), 'F12 к методу менеджера открывает его cf-module документ');
        const commonDefs = await vscode.commands.executeCommand<vscode.Location[]>('vscode.executeDefinitionProvider', doc.uri, new vscode.Position(5, cmName.length + 4));
        assert.ok(commonDefs?.length === 1 && commonDefs[0].uri.scheme === 'cf-module', 'F12 к методу общего модуля');
        const targetDoc = await vscode.workspace.openTextDocument(commonDefs![0].uri);
        const targetEdit = new vscode.WorkspaceEdit();
        targetEdit.insert(commonDefs![0].uri, targetDoc.positionAt(targetDoc.getText().length), '// after jump');
        assert.ok(await vscode.workspace.applyEdit(targetEdit));
        assert.ok(await targetDoc.save());
        assert.equal(api.state().dirty, true, 'модуль, открытый через F12, редактируется и сохраняется в модель');
        const append = new vscode.WorkspaceEdit(); append.insert(doc.uri,doc.positionAt(doc.getText().length),'// flush');
        assert.ok(await vscode.workspace.applyEdit(append));
        const props = await runtime.properties(catalog.id);
        assert.ok(props.properties.some(p => p.name === 'Hierarchical'));
        await runtime.setProperty(catalog.id, 'Comment', 'Host saved comment', api.state().revision);
        assert.equal(api.state().dirty, true);
        await assert.rejects(runtime.close(), /несохран/i);
        await assert.rejects(runtime.setProperty(catalog.id, 'Comment', 'stale', 1), /изменилась/i);
        assert.ok((await runtime.search('PRODUCT')).some(n => n.id === catalog.id));
        assert.ok((await runtime.references(catalog.id)).length > 0);
        assert.equal((await runtime.composition(catalog.id)).length, 3);
        const save = path.join(path.dirname(fixture), 'host-roundtrip.cf');
        await vscode.commands.executeCommand('cfExtension.saveAs', { path: save, format: 'binary' });
        assert.equal(api.state().dirty, false);
        assert.equal(doc.isDirty,false,'configuration save flushes dirty modules');
        await runtime.close();
        await vscode.commands.executeCommand('cfExtension.open', save);
        assert.equal((await runtime.properties(catalog.id)).properties.find(p => p.name === 'Comment')?.value, 'Host saved comment');
        const reopened = await api.openModule(moduleInfos[0].id);
        assert.ok(reopened.getText().includes('// flush'));
        await assert.rejects(api.module(moduleInfos[0].id,doc.uri.path.includes('never') ? 0 : 1),/сессии|сменилась/i);
        const xml = path.join(path.dirname(fixture), 'host-xml');
        await runtime.save(xml, 'xml');
        assert.ok((await fs.stat(path.join(xml, 'Configuration.xml'))).isFile());
        await assert.rejects(runtime.save(xml, 'xml'), /новый|пустой/i);
        await runtime.close();
        console.log('Real Extension Host: activation/open/tree/select/properties/edit/dirty/revision/search/references/composition/save/reopen/XML/diagnostics/completion/definition passed');
    }
    finally {
        api.dispose();
    }
}
