import * as vscode from 'vscode';
import * as path from 'node:path';
import { RpcClient } from './client';
import { InitializeResult, ModelNode, ModelState, PropertyResult } from './protocol';
import { MetadataTree, PropertyPalette } from './navigator';
import { ModuleDocuments } from './moduleDocuments';
import { BslDiagnostics } from './bsl';
import { ModuleInfo, ModuleDocument } from './protocol';
import { TreeIcons } from './treeIcons';
export interface ExtensionApi {
    request<T>(method: string, params?: unknown): Promise<T>;
    notify(method: string, params?: unknown): void;
    onNotification(listener: (method: string, params: unknown) => void): vscode.Disposable;
    onState(listener: (state: ModelState) => void): vscode.Disposable;
    readonly documents: ModuleDocuments;
    openModule(id: string): Promise<vscode.TextDocument>;
    modules(ownerId?: string): Promise<ModuleInfo[]>;
    module(id: string, sessionId: number): Promise<ModuleDocument>;
    setModule(id: string, sessionId: number, version: number, text: string): Promise<{module: ModuleDocument; state: ModelState}>;
    open(path: string): Promise<ModelState>;
    tree(parentId?: string): Promise<ModelNode[]>;
    node(id: string): Promise<ModelNode>;
    properties(id: string): Promise<PropertyResult>;
    setProperty(id: string, name: string, value: unknown, revision: number): Promise<ModelState>;
    search(query: string): Promise<ModelNode[]>;
    references(id: string): Promise<ModelNode[]>;
    composition(id: string): Promise<ModelNode[]>;
    save(path?: string, format?: 'binary' | 'xml'): Promise<ModelState>;
    state(): ModelState;
    close(discardChanges?: boolean): Promise<ModelState>;
    dispose(): void;
}
interface Destination {
    path: string;
    format: 'binary' | 'xml';
}
export async function activate(context: vscode.ExtensionContext): Promise<ExtensionApi> {
    let client: RpcClient | undefined;
    let current: ModelState = { loaded: false, sourcePath: null, dirty: false, revision: 0, sessionId: 0 };
    const notifications = new Set<(method: string, params: unknown) => void>();
    const states = new vscode.EventEmitter<ModelState>();
    let selectionToken = 0;
    let selected: ModelNode | undefined;
    const output = vscode.window.createOutputChannel('Конструктор');
    context.subscriptions.push(output);
    const icons = new TreeIcons(context, output);
    await icons.reload();
    const update = (state: ModelState): ModelState => { if (state.revision >= current.revision) {
        current = state;
        states.fire({...state});
        tree.refresh();
        treeView.title = state.dirty ? 'Метаданные ●' : 'Метаданные';
        // A message suppresses viewsWelcome, including its Open Configuration button.
        treeView.message = state.loaded && state.dirty ? 'Есть несохранённые изменения' : undefined;
        void vscode.commands.executeCommand('setContext', 'cfExtension.loaded', state.loaded);
        void vscode.commands.executeCommand('setContext', 'cfExtension.dirty', state.dirty);
        if (state.loaded) {
            void palette.refresh();
        }
        else {
            selected = undefined;
            palette.clear();
        }
    } return state; };
    async function connect(): Promise<RpcClient> { if (!vscode.workspace.isTrusted) {
        throw new Error('Для открытия конфигурации подтвердите доверие рабочей области.');
    } if (!client) {
        const configured = vscode.workspace.getConfiguration('cfExtension').get<string>('serverPath');
        const candidate = new RpcClient(configured || path.join(context.extensionPath, 'bin', process.platform === 'win32' ? 'model-server.exe' : 'model-server'));
        client = candidate;
        current = { loaded: false, sourcePath: null, dirty: false, revision: 0, sessionId: 0 };
        candidate.onDiagnostic = message => output.append(message);
        candidate.onNotification = (method, params) => { if (client === candidate && method === 'model.changed') {
            update(params as ModelState);
        } if (client === candidate) for (const listener of notifications) listener(method,params); };
        candidate.onExit = error => { if (client === candidate) {
            client = undefined;
            documents.disconnect();
            update({ ...current, loaded: false, sourcePath: null, dirty: false });
            void vscode.window.showErrorMessage(error.message);
        } };
        try {
            const result = await candidate.request<InitializeResult>('initialize');
            if (result.protocolVersion !== 1) {
                throw new Error('Несовместимая версия протокола');
            }
        }
        catch (error) {
            if (client === candidate) {
                client = undefined;
            }
            candidate.dispose();
            throw error;
        }
    } return client; }
    async function request<T>(method: string, params: unknown = {}): Promise<T> { return (await connect()).request<T>(method, params); }
    const api: ExtensionApi = {
        request,
        notify(method, params) { const active = client; if (active) { active.notify(method, params); } else { void connect().then(c => c.notify(method, params)).catch(() => {}); } },
        onNotification(listener) { notifications.add(listener); return new vscode.Disposable(() => { notifications.delete(listener); }); },
        onState(listener) { return states.event(listener); },
        get documents() { return documents; },
        openModule(id) { return documents.openModule(id); },
        modules(ownerId) { return request('model.modules',ownerId ? {ownerId} : {}); },
        module(id,sessionId) { return request('model.module',{id,sessionId}); },
        async setModule(id,sessionId,version,text) { const result=await request<{module:ModuleDocument;state:ModelState}>('model.setModule',{id,sessionId,version,text}); update(result.state); return result; },
        async open(filePath) { const state=update(await request<ModelState>('model.open',{path:filePath})); await documents.retire(); return state; },
        tree(parentId) { return request('model.tree', parentId ? { parentId } : {}); }, node(id) { return request('model.node', { id }); }, properties(id) { return request('model.properties', { id }); },
        async setProperty(id, name, value, revision) { return update(await request<ModelState>('model.setProperty', { id, name, value, revision })); }, search(query) { return request('model.search', { query }); }, references(id) { return request('model.references', { id }); }, composition(id) { return request('model.composition', { id }); },
        async save(filePath, format) { await documents.flush(); return update(await request<ModelState>('model.save', { ...(filePath ? { path: filePath } : {}), ...(format ? { format } : {}), revision: current.revision })); },
        async close(discardChanges) { if (documents.dirtyDocuments().length && !discardChanges) throw new Error('Есть несохранённые изменения модулей'); const state=update(await request<ModelState>('model.close', { discardChanges })); await documents.retire(); return state; }, state() { return { ...current }; }, dispose() { const old = client; client = undefined; old?.dispose(); }
    };
    const documents = new ModuleDocuments(api);
    const bsl = new BslDiagnostics(api, documents);
    context.subscriptions.push(documents, bsl, states);
    const tree = new MetadataTree(api, icons);
    context.subscriptions.push(vscode.workspace.onDidChangeConfiguration(event => { if (event.affectsConfiguration('cfExtension.platformPath')) { void icons.reload().then(() => tree.refresh()); } }));
    const treeView = vscode.window.createTreeView('cfExtension.metadata', { treeDataProvider: tree, showCollapseAll: true });
    const palette = new PropertyPalette(api);
    context.subscriptions.push(tree, treeView, { dispose: () => api.dispose() }, vscode.window.registerWebviewViewProvider('cfExtension.properties', palette));
    async function select(value?: ModelNode | string): Promise<void> { const id = typeof value === 'string' ? value : value?.id; if (!id) {
        return;
    } const token = ++selectionToken; const revision = current.revision; const node = typeof value === 'object' ? value : await api.node(id); if (node.kind === 'group') {
        return;
    } if (node.kind === 'module') { return; } if (token !== selectionToken || revision !== current.revision) return; if (node.kind === 'record') {
        void vscode.window.showInformationMessage('Ссылка найдена в записи контейнера; отдельная палитра для неё недоступна.');
        return;
    } selected = node; await vscode.commands.executeCommand('cfExtension.properties.focus'); if (token !== selectionToken || current.revision !== revision) {
        return;
    } await palette.select(id); }
    async function chooseSave(destination?: Destination): Promise<ModelState | undefined> { if (destination) {
        return api.save(destination.path, destination.format);
    } const choice = await vscode.window.showQuickPick([{ label: 'Бинарный файл CF / CFE / EPF', format: 'binary' as const }, { label: 'XML в новый или пустой каталог', description: 'Непустой каталог не изменяется. XML-источник требует явного назначения.', format: 'xml' as const }], { title: 'Сохранить конфигурацию' }); if (!choice) {
        return;
    } if (choice.format === 'xml') {
        const folder = await vscode.window.showSaveDialog({ title: 'Новый или пустой XML-каталог', saveLabel: 'Экспортировать XML' });
        return folder ? api.save(folder.fsPath, 'xml') : undefined;
    } const file = await vscode.window.showSaveDialog({ title: 'Сохранить бинарный файл', filters: { 'Конфигурация 1С': ['cf', 'cfe', 'epf'] } }); return file ? api.save(file.fsPath, 'binary') : undefined; }
    async function dirtyGuard(): Promise<boolean> { if (!current.dirty && !documents.dirtyDocuments().length) {
        return true;
    } const answer = await vscode.window.showWarningMessage('Есть несохранённые изменения конфигурации.', { modal: true }, 'Сохранить', 'Отбросить', 'Отмена'); if (answer === 'Сохранить') {
        if (current.sourcePath && !isXmlSource(current.sourcePath)) {
            await api.save();
            return true;
        }
        return !!await chooseSave();
    } return answer === 'Отбросить'; }
    async function pick(nodes: ModelNode[], title: string, empty: string): Promise<ModelNode | undefined> { if (!nodes.length) {
        void vscode.window.showInformationMessage(empty);
        return;
    } const choice = await vscode.window.showQuickPick(nodes.map(n => ({ label: n.label, description: n.kind, node: n })), { title }); return choice?.node; }
    async function reveal(node: ModelNode): Promise<void> { if (node.kind === 'record') {
        await select(node);
        return;
    } await treeView.reveal(node, { select: true, focus: true, expand: true }); await select(node); }
    function register(name: string, handler: (...args: any[]) => Promise<unknown>): void { context.subscriptions.push(vscode.commands.registerCommand('cfExtension.' + name, async (...args: unknown[]) => { try {
        return await handler(...args);
    }
    catch (error) {
        void vscode.window.showErrorMessage(`Конструктор: ${error instanceof Error ? error.message : String(error)}`);
        throw error;
    } })); }
    register('open', async (filePath?: string) => { if (!filePath) {
        const files = await vscode.window.showOpenDialog({ canSelectMany: false, filters: { 'Конфигурация 1С': ['cf', 'cfe', 'epf', 'xml'] } });
        filePath = files?.[0]?.fsPath;
    } if (!filePath || !await dirtyGuard()) {
        return;
    } const state = update(await request<ModelState>('model.open', { path: filePath, discardChanges: current.dirty })); await documents.retire(); selectionToken++; selected = undefined; palette.clear(); return state; });
    register('openModule', async (value: ModelNode | string) => { const id=typeof value === 'string' ? value : value.id; const doc=await api.openModule(id.startsWith('module:') ? id.slice(7) : id); await vscode.window.showTextDocument(doc,{preview:false}); return doc; });
    register('select', select);
    treeView.onDidChangeSelection(event => { if (event.selection[0]) {
        void select(event.selection[0]).catch(e => vscode.window.showErrorMessage(String(e)));
    } }, undefined, context.subscriptions);
    register('save', async () => { if (!current.loaded) {
        throw new Error('Откройте конфигурацию');
    } return current.sourcePath && !isXmlSource(current.sourcePath) ? api.save() : chooseSave(); });
    register('saveAs', chooseSave);
    register('close', async () => { if (!await dirtyGuard()) {
        return;
    } return api.close(true); });
    register('search', async (query?: string) => { query = query ?? await vscode.window.showInputBox({ title: 'Поиск объекта по имени или синониму', prompt: 'Поиск метаданных без учёта регистра' }); if (query === undefined) {
        return;
    } if (!query.trim()) {
        void vscode.window.showInformationMessage('Введите имя или синоним объекта.');
        return;
    } const nodes = await api.search(query); const choice = await pick(nodes, 'Результаты поиска', 'Объекты не найдены.'); if (choice) {
        await reveal(choice);
    } return nodes; });
    register('findReferences', async (value?: ModelNode | string) => { const id = typeof value === 'string' ? value : value?.id ?? selected?.id; if (!id) {
        throw new Error('Выберите объект в дереве.');
    } const nodes = await api.references(id); const choice = await pick(nodes, 'Ссылки GUID в записях (не семантический поиск BSL)', 'Ссылки GUID не найдены. Это не семантический поиск BSL.'); if (choice) {
        await reveal(choice);
    } return nodes; });
    register('composition', async (value?: ModelNode | string) => { const id = typeof value === 'string' ? value : value?.id ?? selected?.id; if (!id) {
        throw new Error('Выберите объект в дереве.');
    } const nodes = await api.composition(id); const choice = await pick(nodes, 'Состав: реквизиты и табличные части', 'Вложенный состав отсутствует.'); if (choice) {
        await reveal(choice);
    } return nodes; });
    update(current);
    return api;
}
function isXmlSource(filePath: string): boolean { return path.extname(filePath).toLowerCase() === '.xml' || !['.cf', '.cfe', '.epf'].includes(path.extname(filePath).toLowerCase()); }
