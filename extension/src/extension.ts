import * as vscode from 'vscode';
import * as path from 'node:path';
import { RpcClient } from './client';
import { InitializeResult, ModelNode, ModelState } from './protocol';
export interface ExtensionApi {
    open(path: string): Promise<ModelState>;
    tree(parentId?: string): Promise<ModelNode[]>;
    state(): ModelState;
    dispose(): void;
}
export function activate(context: vscode.ExtensionContext): ExtensionApi {
    let client: RpcClient | undefined;
    let current: ModelState = { loaded: false, sourcePath: null, dirty: false, revision: 0 };
    const output = vscode.window.createOutputChannel('Конструктор');
    context.subscriptions.push(output);
    async function connect(): Promise<RpcClient> {
        if (!vscode.workspace.isTrusted) { throw new Error('Для открытия конфигурации подтвердите доверие рабочей области.'); }
        if (!client) {
            const configured = vscode.workspace.getConfiguration('cfExtension').get<string>('serverPath');
            const candidate = new RpcClient(configured || path.join(context.extensionPath,'bin',process.platform === 'win32' ? 'model-server.exe' : 'model-server'));
            client = candidate;
            candidate.onDiagnostic = message => output.append(message);
            candidate.onExit = error => { if (client === candidate) { client = undefined; current = { ...current, loaded:false }; void vscode.window.showErrorMessage(error.message); } };
            candidate.onNotification = (method,params) => { if (method === 'model.changed') { current = params as ModelState; } };
            try { const result = await candidate.request<InitializeResult>('initialize'); if (result.protocolVersion !== 1) { throw new Error('Несовместимая версия протокола'); } }
            catch (error) { if (client === candidate) { client = undefined; } candidate.dispose(); throw error; }
        }
        return client;
    }
    const api: ExtensionApi = {
        async open(filePath) { current = await (await connect()).request<ModelState>('model.open',{path:filePath}); return current; },
        async tree(parentId) { return (await connect()).request<ModelNode[]>('model.tree',parentId ? {parentId} : {}); },
        state() { return {...current}; },
        dispose() { const previous=client; client=undefined; previous?.dispose(); }
    };
    context.subscriptions.push({dispose:()=>api.dispose()});
    context.subscriptions.push(vscode.commands.registerCommand('cfExtension.open',async (filePath?: string) => {
        try {
            if (!filePath) { const files = await vscode.window.showOpenDialog({canSelectMany:false,filters:{'Конфигурация 1С':['cf','cfe','epf','xml']}}); filePath=files?.[0]?.fsPath; }
            if (!filePath) { return; }
            const state = await api.open(filePath); const roots = await api.tree();
            void vscode.window.showInformationMessage(`Открыто: ${roots[0]?.label ?? state.sourcePath}`); return state;
        } catch (error) { void vscode.window.showErrorMessage(`Не удалось открыть конфигурацию: ${String(error)}`); throw error; }
    }));
    return api;
}
