import * as vscode from 'vscode';
import type { ExtensionApi } from './extension';
import { ModuleDocuments } from './moduleDocuments';

interface PublishParams {
    uri: string;
    version?: number;
    diagnostics: {
        range: { start: { line: number; character: number }; end: { line: number; character: number } };
        severity?: number;
        message: string;
        source?: string;
    }[];
}

/** Мост диагностики BSL: lifecycle документов → тот же JSON-RPC канал, publishDiagnostics → коллекция Problems.
 *  Работает только со своими cf-module документами; запоздалые версии и старые сессии игнорируются. */
export class BslDiagnostics implements vscode.Disposable {
    private readonly collection = vscode.languages.createDiagnosticCollection('cf-bsl');
    /** Документы, по которым отправлен didOpen (didChange без open не отправляется). */
    private readonly opened = new Set<string>();
    private readonly disposables: vscode.Disposable[] = [];
    constructor(private readonly api: ExtensionApi, private readonly documents: ModuleDocuments) {
        this.disposables.push(
            documents.onDidOpen(document => this.didOpen(document)),
            documents.onDidChangeContent(document => this.didChange(document)),
            documents.onDidClose(uri => this.didClose(uri)),
            documents.onDidRetireAll(() => this.clearAll()),
            api.onNotification((method, params) => {
                if (method === 'textDocument/publishDiagnostics') {
                    this.publish(params as PublishParams);
                }
            }),
            this.collection,
        );
    }
    private didOpen(document: vscode.TextDocument): void {
        const identity = this.documents.identity(document.uri);
        if (!identity) {
            return;
        }
        this.opened.add(document.uri.toString());
        this.api.notify('textDocument/didOpen', {
            uri: document.uri.toString(),
            languageId: document.languageId,
            version: document.version,
            text: document.getText(),
            moduleId: identity.id,
            sessionId: identity.sessionId,
        });
    }
    private didChange(document: vscode.TextDocument): void {
        const key = document.uri.toString();
        if (!this.opened.has(key) || !this.documents.identity(document.uri)) {
            return;
        }
        this.api.notify('textDocument/didChange', { uri: key, version: document.version, text: document.getText() });
    }
    private didClose(uri: vscode.Uri): void {
        this.opened.delete(uri.toString());
        this.collection.delete(uri);
        this.api.notify('textDocument/didClose', { uri: uri.toString() });
    }
    private clearAll(): void {
        this.opened.clear();
        this.collection.clear();
    }
    private publish(params: PublishParams): void {
        const uri = vscode.Uri.parse(params.uri);
        // Старая сессия или уже снятый документ — не трогаем текущий буфер.
        if (!this.documents.identity(uri)) {
            return;
        }
        const document = vscode.workspace.textDocuments.find(d => d.uri.toString() === params.uri);
        if (params.version !== undefined && document && document.version !== params.version) {
            return;
        }
        const diagnostics = params.diagnostics.map(d => {
            const diagnostic = new vscode.Diagnostic(
                new vscode.Range(d.range.start.line, d.range.start.character, d.range.end.line, d.range.end.character),
                d.message,
                d.severity === 2 ? vscode.DiagnosticSeverity.Warning : vscode.DiagnosticSeverity.Error,
            );
            diagnostic.source = d.source;
            return diagnostic;
        });
        this.collection.set(uri, diagnostics);
    }
    dispose(): void {
        for (const disposable of this.disposables) {
            disposable.dispose();
        }
    }
}
