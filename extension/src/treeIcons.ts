import * as vscode from 'vscode';
import * as path from 'node:path';
import { discoverPlatform, loadPlatformIcons, IconFiles } from './platformIcons';
import { ModelNode } from './protocol';
export class TreeIcons {
    private files = new Map<string,IconFiles>();
    constructor(private readonly context:vscode.ExtensionContext,private readonly output:vscode.OutputChannel) {}
    async reload():Promise<void> {
        this.files.clear();
        try {
            const configured=vscode.workspace.getConfiguration('cfExtension').get<string>('platformPath') || undefined;
            const directory=await discoverPlatform(configured);
            const icons=await loadPlatformIcons(directory,path.join(this.context.globalStorageUri.fsPath,'platform-icons'));
            this.files=icons.files;
            this.output.appendLine(directory?`Иконки 1С: ${directory}; вариантов загружено: ${icons.files.size}.`:'Установленная 1С не найдена: используются собственные SVG.');
            for(const diagnostic of icons.diagnostics)this.output.appendLine(`Иконки 1С: ${diagnostic}`);
        } catch(error) { this.output.appendLine(`Иконки 1С недоступны, используются SVG: ${String(error)}`); }
    }
    get(node:ModelNode): vscode.Uri | {light:vscode.Uri;dark:vscode.Uri} {
        const kind=node.metadataKind || node.kind;
        const original=this.files.get(`${kind}:${node.kind==='group'?'group':'object'}`);
        if(original)return {light:vscode.Uri.file(original.light),dark:vscode.Uri.file(original.dark)};
        let family='object';
        if(kind==='Configuration')family='root';
        else if(kind==='Common'||node.kind==='group')family='folder';
        else if(/Register|DataSource/.test(kind))family='register';
        else if(/Catalog|Chart/.test(kind))family='catalog';
        else if(/Document|Report|Processor|Template/.test(kind))family='document';
        else if(/Form/.test(kind))family='form';
        else if(/Attribute|Resource|Dimension|Parameter|Column|Flag/.test(kind))family='attribute';
        else if(/TabularSection/.test(kind))family='table';
        else if(/Module|Command|Method|Operation/.test(kind))family='module';
        else if(/Enum/.test(kind))family='enum';
        return vscode.Uri.joinPath(this.context.extensionUri,'media','tree',family+'.svg');
    }
}
