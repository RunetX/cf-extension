import * as vscode from 'vscode';
import type { ExtensionApi } from './extension';
import { ModuleDocuments } from './moduleDocuments';

interface CompletionItemDto {
    label: string;
    kind: number;
    detail: string;
    insertText: string;
}

type DefinitionDto =
    | { kind: 'line'; uri: string; line: number; character: number }
    | { kind: 'module'; module: string; line: number | null }
    | { kind: 'metadata'; class: string; name: string; id: string };

/** Подсказки (Ctrl+Пробел) и переходы к определению (F12) через тот же JSON-RPC канал.
 *  Работает только со своими cf-module документами; запоздавшие ответы после смены модели игнорируются. */
export class BslNavigation implements vscode.Disposable {
    private readonly disposables: vscode.Disposable[] = [];
    constructor(private readonly api: ExtensionApi, private readonly documents: ModuleDocuments) {
        const selector: vscode.DocumentSelector = { scheme: 'cf-module', language: 'bsl' };
        this.disposables.push(
            vscode.languages.registerCompletionItemProvider(selector, { provideCompletionItems: (d, p, t) => this.completion(d, p, t) }, '.'),
            vscode.languages.registerDefinitionProvider(selector, { provideDefinition: (d, p, t) => this.definition(d, p, t) }),
        );
    }

    private async completion(document: vscode.TextDocument, position: vscode.Position, token: vscode.CancellationToken): Promise<vscode.CompletionItem[] | undefined> {
        if (!this.documents.identity(document.uri)) {
            return undefined;
        }
        const items = await this.api.request<CompletionItemDto[]>('textDocument/completion', {
            textDocument: { uri: document.uri.toString() },
            position: { line: position.line, character: position.character },
        });
        if (!Array.isArray(items) || token.isCancellationRequested || !this.documents.identity(document.uri)) {
            return undefined;
        }
        const range = document.getWordRangeAtPosition(position) ?? new vscode.Range(position, position);
        return items.map(item => {
            const completion = new vscode.CompletionItem(item.label, this.kind(item.kind));
            completion.detail = item.detail;
            // "( ?)" — место курсора: превращаем в сниппет, остальное вставляется как есть.
            const snippet = item.insertText.endsWith('(?)') ? item.insertText.slice(0, -3) + '($0)' : undefined;
            completion.insertText = snippet !== undefined ? new vscode.SnippetString(snippet) : item.insertText;
            completion.range = range;
            return completion;
        });
    }

    private async definition(document: vscode.TextDocument, position: vscode.Position, token: vscode.CancellationToken): Promise<vscode.Definition | null> {
        if (!this.documents.identity(document.uri)) {
            return null;
        }
        const target = await this.api.request<DefinitionDto | null>('textDocument/definition', {
            textDocument: { uri: document.uri.toString() },
            position: { line: position.line, character: position.character },
        });
        if (!target || token.isCancellationRequested || !this.documents.identity(document.uri)) {
            return null;
        }
        if (target.kind === 'line') {
            return new vscode.Location(vscode.Uri.parse(target.uri), new vscode.Position(target.line, target.character));
        }
        if (target.kind === 'module') {
            // Целевой модуль ещё не открыт: открываем через общий ModuleDocuments — канонический URI,
            // пригоден к редактированию и сохранению; несуществующую цель сервер уже отклонил.
            const doc = await this.api.openModule(target.module);
            if (token.isCancellationRequested) {
                return null;
            }
            await vscode.window.showTextDocument(doc, { preview: false });
            return new vscode.Location(doc.uri, new vscode.Position(target.line ?? 0, 0));
        }
        // Имя метаданных: настоящий объект дерева и его палитра, не открываемый файл.
        await vscode.commands.executeCommand('cfExtension.reveal', target.id);
        return [];
    }

    private kind(kind: number): vscode.CompletionItemKind {
        switch (kind) {
            case 2: return vscode.CompletionItemKind.Method;
            case 5: return vscode.CompletionItemKind.Field;
            case 6: return vscode.CompletionItemKind.Variable;
            case 7: return vscode.CompletionItemKind.Class;
            case 9: return vscode.CompletionItemKind.Module;
            case 14: return vscode.CompletionItemKind.Keyword;
            default: return vscode.CompletionItemKind.Value;
        }
    }

    dispose(): void {
        for (const disposable of this.disposables) {
            disposable.dispose();
        }
    }
}
