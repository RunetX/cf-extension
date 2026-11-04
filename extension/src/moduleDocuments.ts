import * as vscode from 'vscode';
import { randomUUID } from 'node:crypto';
import { ModuleDocument, ModuleInfo, ModelState } from './protocol';
export interface ModuleActions {
    state(): ModelState;
    modules(ownerId?: string): Promise<ModuleInfo[]>;
    module(id: string, sessionId: number): Promise<ModuleDocument>;
    setModule(id: string, sessionId: number, version: number, text: string): Promise<{module: ModuleDocument; state: ModelState}>;
}
interface Binding { uri: vscode.Uri; module: ModuleDocument; retired: boolean; }
/** Owns editor identities, never local file paths. No implicit writes during disposal. */
export class ModuleDocuments implements vscode.FileSystemProvider, vscode.Disposable {
    private connection = randomUUID();
    private readonly bindings = new Map<string, Binding>();
    private readonly changed = new vscode.EventEmitter<vscode.FileChangeEvent[]>();
    readonly onDidChangeFile = this.changed.event;
    private readonly registration: vscode.Disposable;
    constructor(private readonly api: ModuleActions) {
        this.registration = vscode.workspace.registerFileSystemProvider('cf-module', this, {isCaseSensitive:true});
    }
    identity(uri: vscode.Uri): ModuleDocument | undefined {
        const b = this.bindings.get(uri.toString());
        return b && !b.retired && b.module.sessionId === this.api.state().sessionId && this.api.state().loaded ? {...b.module} : undefined;
    }
    async openModule(id: string): Promise<vscode.TextDocument> {
        const session = this.api.state().sessionId;
        const existing = [...this.bindings.values()].find(b => !b.retired && b.module.id === id && b.module.sessionId === session);
        if (existing) return vscode.workspace.openTextDocument(existing.uri);
        const module = await this.api.module(id,session);
        if (session !== this.api.state().sessionId || !this.api.state().loaded) throw vscode.FileSystemError.Unavailable('Конфигурация сменилась');
        const label = module.label.replace(/[\\/:*?"<>|]/g,'_');
        const uri = vscode.Uri.from({scheme:'cf-module',authority:this.connection,path:`/${session}/${Buffer.from(id,'utf8').toString('hex')}/${label}.bsl`});
        this.bindings.set(uri.toString(), {uri,module,retired:false});
        const document = await vscode.workspace.openTextDocument(uri);
        return vscode.languages.setTextDocumentLanguage(document,'bsl');
    }
    dirtyDocuments(): vscode.TextDocument[] {
        return vscode.workspace.textDocuments.filter(d => d.isDirty && !!this.identity(d.uri));
    }
    async flush(): Promise<void> {
        for (const document of this.dirtyDocuments()) {
            if (!await document.save()) throw new Error(`Не удалось сохранить модуль ${document.fileName}`);
        }
    }
    /** Call only after a successful model replacement; failures preserve dirty text. */
    async retire(): Promise<void> {
        const old = [...this.bindings.values()].filter(b => !b.retired);
        old.forEach(b => b.retired = true);
        for (const b of old) {
            // Revert reads our retained applied snapshot; it never writes into the new model.
            await vscode.commands.executeCommand('workbench.action.files.revert', b.uri);
            const tabs = vscode.window.tabGroups.all.flatMap(g => g.tabs).filter(t => t.input instanceof vscode.TabInputText && t.input.uri.toString() === b.uri.toString());
            for (const tab of tabs) await vscode.window.tabGroups.close(tab,true);
        }
    }
    disconnect(): void { this.connection = randomUUID(); this.bindings.forEach(b => b.retired=true); }
    watch(): vscode.Disposable { return new vscode.Disposable(() => {}); }
    stat(uri: vscode.Uri): vscode.FileStat { const b=this.binding(uri); return {type:vscode.FileType.File,ctime:0,mtime:b.module.version,size:Buffer.byteLength(b.module.text)}; }
    readFile(uri: vscode.Uri): Uint8Array { return Buffer.from(this.binding(uri).module.text,'utf8'); }
    async writeFile(uri: vscode.Uri, content: Uint8Array): Promise<void> {
        const b = this.binding(uri);
        if (!this.identity(uri)) throw vscode.FileSystemError.Unavailable('Документ старой сессии; скопируйте текст в новый модуль');
        const result = await this.api.setModule(b.module.id,b.module.sessionId,b.module.version,Buffer.from(content).toString('utf8'));
        b.module = result.module;
        this.changed.fire([{type:vscode.FileChangeType.Changed,uri}]);
    }
    readDirectory(): [string,vscode.FileType][] { return []; }
    createDirectory(): never { throw vscode.FileSystemError.NoPermissions(); }
    delete(): never { throw vscode.FileSystemError.NoPermissions(); }
    rename(): never { throw vscode.FileSystemError.NoPermissions(); }
    private binding(uri: vscode.Uri): Binding { const b=this.bindings.get(uri.toString()); if (!b) throw vscode.FileSystemError.FileNotFound(uri); return b; }
    dispose(): void { this.disconnect(); this.registration.dispose(); this.changed.dispose(); }
}
