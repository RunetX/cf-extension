import * as vscode from 'vscode';
import * as crypto from 'node:crypto';
import { TreeIcons } from './treeIcons';
import { ModelNode, ModelProperty, ModelState, PropertyResult } from './protocol';
export interface NavigatorActions {
    tree(parentId?: string): Promise<ModelNode[]>;
    node(id: string): Promise<ModelNode>;
    properties(id: string): Promise<PropertyResult>;
    setProperty(id: string, name: string, value: unknown, revision: number): Promise<ModelState>;
    state(): ModelState;
}
export class MetadataTree implements vscode.TreeDataProvider<ModelNode> {
    private readonly changed = new vscode.EventEmitter<ModelNode | undefined>();
    readonly onDidChangeTreeData = this.changed.event;
    private parents = new Map<string, string>();
    private nodes = new Map<string, ModelNode>();
    constructor(private readonly api: NavigatorActions, private readonly icons: TreeIcons) { }
    refresh(): void { this.parents.clear(); this.nodes.clear(); this.changed.fire(undefined); }
    getTreeItem(n: ModelNode): vscode.TreeItem {
        const state = n.hasChildren ? (!n.parentId ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.Collapsed) : vscode.TreeItemCollapsibleState.None;
        const item = new vscode.TreeItem(n.label, state);
        item.id = n.id;
        item.tooltip = n.typeLabel ? `${n.label} — ${n.typeLabel}` : n.label;
        item.contextValue = n.kind === 'group' ? 'group' : n.kind === 'record' ? 'record' : 'metadata';
        if (n.kind !== 'group') item.command = { command: 'cfExtension.select', title: 'Свойства', arguments: [n] };
        item.iconPath = this.icons.get(n);
        return item;
    }
    async getChildren(n?: ModelNode): Promise<ModelNode[]> { if (!this.api.state().loaded) {
        return [];
    } try {
        const nodes = await this.api.tree(n?.id);
        for (const node of nodes) {
            this.nodes.set(node.id, node);
            if (node.parentId) {
                this.parents.set(node.id, node.parentId);
            }
        }
        return nodes;
    }
    catch (error) {
        void vscode.window.showErrorMessage(`Не удалось загрузить дерево: ${String(error)}`);
        return [];
    } }
    async getParent(n: ModelNode): Promise<ModelNode | undefined> {
        const parent = n.parentId ?? this.parents.get(n.id);
        if (!parent) return undefined;
        let node = this.nodes.get(parent);
        if (!node) { node = await this.api.node(parent); this.nodes.set(parent, node); }
        return node;
    }
    dispose(): void { this.changed.dispose(); }
}
export class PropertyPalette implements vscode.WebviewViewProvider {
    private view?: vscode.WebviewView;
    private selected?: string;
    private version = 0;
    private rendered?: {
        id: string;
        revision: number;
        properties: ModelProperty[];
    };
    private busy = false;
    constructor(private readonly api: NavigatorActions) { }
    resolveWebviewView(view: vscode.WebviewView): void { this.view = view; view.webview.options = { enableScripts: true, localResourceRoots: [] }; view.webview.html = html(); view.webview.onDidReceiveMessage(message => { void this.receive(message); }); view.onDidDispose(() => { if (this.view === view) {
        this.view = undefined;
    } }); void this.refresh(); }
    async select(id: string): Promise<void> { this.selected = id; await this.refresh(); }
    clear(): void { this.selected = undefined; this.rendered = undefined; this.version++; void this.view?.webview.postMessage({ type: 'empty', text: 'Выберите объект в дереве.' }); }
    async refresh(): Promise<void> { const token = ++this.version; const id = this.selected; if (!id || !this.api.state().loaded) {
        this.clear();
        return;
    } if (this.busy) {
        return;
    } void this.view?.webview.postMessage({ type: 'loading', text: 'Загрузка свойств…' }); const revision = this.api.state().revision; try {
        const result = await this.api.properties(id);
        if (token !== this.version || id !== this.selected || revision !== this.api.state().revision) {
            return;
        }
        this.rendered = { ...result, revision };
        void this.view?.webview.postMessage({ type: 'properties', ...result, revision, dirty: this.api.state().dirty });
    }
    catch (error) {
        if (token === this.version) {
            void this.view?.webview.postMessage({ type: 'error', text: String(error) });
        }
    } }
    private async receive(message: unknown): Promise<void> { if (!message || typeof message !== 'object') {
        return;
    } const m = message as Record<string, unknown>; if (m.type === 'ready') {
        await this.refresh();
        return;
    } const snapshot = this.rendered; if (m.type !== 'edit' || !snapshot || m.id !== snapshot.id || m.id !== this.selected || m.revision !== snapshot.revision || m.revision !== this.api.state().revision || typeof m.name !== 'string' || this.busy) {
        void this.view?.webview.postMessage({ type: 'editError', text: 'Палитра устарела. Выберите объект заново.' });
        return;
    } const p = snapshot.properties.find(p => p.name === m.name); if (!p?.editable || !validValue(p, m.value)) {
        void this.view?.webview.postMessage({ type: 'editError', text: 'Недопустимое значение свойства.' });
        return;
    } this.busy = true; try {
        await this.api.setProperty(snapshot.id, p.name, m.value, snapshot.revision);
        this.busy = false;
        await this.refresh();
    }
    catch (error) {
        this.busy = false;
        void this.view?.webview.postMessage({ type: 'editError', text: String(error) });
    } }
}
function validValue(p: ModelProperty, v: unknown): boolean { return p.kind === 'boolean' ? typeof v === 'boolean' : p.kind === 'number' ? typeof v === 'number' && Number.isFinite(v) : p.kind === 'enum' ? typeof v === 'string' && !!p.options?.includes(v) : p.kind === 'string' && typeof v === 'string'; }
function html(): string {
    const nonce = crypto.randomBytes(24).toString('base64');
    return `<!DOCTYPE html><html lang="ru"><head><meta charset="UTF-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'nonce-${nonce}'; script-src 'nonce-${nonce}';"><meta name="viewport" content="width=device-width,initial-scale=1"><style nonce="${nonce}">body{color:var(--vscode-foreground);background:var(--vscode-sideBar-background);font-family:var(--vscode-font-family);font-size:var(--vscode-font-size);padding:12px}label{display:block;margin:12px 0 5px}input,select,textarea{box-sizing:border-box;width:100%;background:var(--vscode-input-background);color:var(--vscode-input-foreground);border:1px solid var(--vscode-input-border,transparent);padding:5px}input[type=checkbox]{width:auto}button{margin-top:5px;padding:5px 10px;background:var(--vscode-button-background);color:var(--vscode-button-foreground);border:0;cursor:pointer}button:hover{background:var(--vscode-button-hoverBackground)}:focus-visible{outline:1px solid var(--vscode-focusBorder)}#error{color:var(--vscode-errorForeground);white-space:pre-wrap}pre{white-space:pre-wrap;overflow-wrap:anywhere}h2{font-size:1em}</style></head><body><p id="status" role="status">Выберите объект в дереве.</p><p id="error" role="alert"></p><main id="fields"></main><script nonce="${nonce}">
const vscode=acquireVsCodeApi();const status=document.getElementById('status'),error=document.getElementById('error'),fields=document.getElementById('fields');let snapshot;
window.addEventListener('message',event=>{const m=event.data;if(m.type==='editError'||m.type==='error'){error.textContent=m.text;for(const b of fields.querySelectorAll('button'))b.disabled=false;return;}if(m.type==='loading'){status.textContent=m.text;return;}if(m.type==='empty'){snapshot=undefined;fields.replaceChildren();status.textContent=m.text;error.textContent='';return;}if(m.type!=='properties')return;snapshot=m;fields.replaceChildren();error.textContent='';status.textContent=(m.dirty?'● Есть несохранённые изменения. ':'')+'Свойства · версия '+m.revision;for(const p of m.properties){const row=document.createElement('section'),label=document.createElement('label');label.textContent=p.label||p.name;const inputId='property-'+p.name;label.htmlFor=inputId;row.append(label);if(!p.editable){const value=document.createElement('pre');value.id=inputId;value.textContent=String(p.value??'')+' (только чтение)';row.append(value);}else{let input;if(p.kind==='enum'){input=document.createElement('select');for(const value of p.options||[]){const option=document.createElement('option');option.value=value;option.textContent=value;input.append(option);}input.value=String(p.value);}else if(p.kind==='boolean'){input=document.createElement('input');input.type='checkbox';input.checked=p.value;}else{input=document.createElement(p.name==='Comment'?'textarea':'input');if(p.kind==='number')input.type='number';input.value=String(p.value??'');}input.id=inputId;row.append(input);const button=document.createElement('button');button.textContent='Применить';button.setAttribute('aria-label','Применить: '+(p.label||p.name));button.addEventListener('click',()=>{const value=p.kind==='boolean'?input.checked:p.kind==='number'?Number(input.value):input.value;for(const b of fields.querySelectorAll('button'))b.disabled=true;error.textContent='';vscode.postMessage({type:'edit',id:m.id,revision:m.revision,name:p.name,value});});row.append(button);}fields.append(row);}});vscode.postMessage({type:'ready'});
</script></body></html>`;
}
