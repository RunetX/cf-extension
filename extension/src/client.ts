import { ChildProcessWithoutNullStreams, spawn } from 'node:child_process';
const FRAME_LIMIT = 16 * 1024 * 1024;
interface Pending { resolve(value: unknown): void; reject(error: Error): void; timer: NodeJS.Timeout }
export class RpcClient {
    private readonly child: ChildProcessWithoutNullStreams;
    private readonly pending = new Map<number, Pending>();
    private buffer: Buffer = Buffer.alloc(0);
    private nextId = 0;
    private stopped = false;
    onNotification?: (method: string, params: unknown) => void;
    onExit?: (error: Error) => void;
    onDiagnostic?: (message: string) => void;
    constructor(path: string, args: string[] = [], private readonly timeoutMs = 30000) {
        this.child = spawn(path, args, { stdio: 'pipe', shell: false, windowsHide: true });
        this.child.stdout.on('data', (chunk: Buffer) => this.receive(chunk));
        this.child.stderr.on('data', (chunk: Buffer) => this.onDiagnostic?.(chunk.toString('utf8')));
        this.child.on('error', error => this.fail(error));
        this.child.on('exit', (code, signal) => this.fail(new Error(`Сервер завершился (exit ${code}, ${signal ?? ''}). Несохранённое состояние потеряно.`)));
        this.child.stdin.on('error', error => this.fail(error));
    }
    request<T = unknown>(method: string, params: unknown = {}): Promise<T> {
        if (this.stopped) { return Promise.reject(new Error('Сервер завершился')); }
        const id = ++this.nextId;
        return new Promise<T>((resolve, reject) => {
            const timer = setTimeout(() => { this.pending.delete(id); reject(new Error(`Таймаут запроса ${method}`)); }, this.timeoutMs);
            this.pending.set(id, { resolve: value => resolve(value as T), reject, timer });
            const body = Buffer.from(JSON.stringify({ jsonrpc: '2.0', id, method, params }), 'utf8');
            if (body.length > FRAME_LIMIT) { this.pending.delete(id); clearTimeout(timer); reject(new Error('Frame exceeds 16 MiB')); return; }
            this.child.stdin.write(Buffer.concat([Buffer.from(`Content-Length: ${body.length}\r\n\r\n`), body]));
        });
    }
    private receive(chunk: Buffer): void {
        if (this.stopped) { return; }
        this.buffer = Buffer.concat([this.buffer, chunk]);
        try {
            while (true) {
                const end = this.buffer.indexOf('\r\n\r\n');
                if (end < 0) { if (this.buffer.length > 8192) { throw new Error('Oversized RPC header'); } return; }
                if (end > 8192) { throw new Error('Oversized RPC header'); }
                const headers = this.buffer.subarray(0,end).toString('ascii').split('\r\n');
                const lengths = headers.filter(header => /^Content-Length:/i.test(header));
                if (lengths.length !== 1 || !/^Content-Length:\s*\d+\s*$/i.test(lengths[0])) { throw new Error('Invalid Content-Length'); }
                const length = Number(lengths[0].split(':')[1]);
                if (!Number.isSafeInteger(length) || length > FRAME_LIMIT) { throw new Error('Frame exceeds 16 MiB'); }
                if (this.buffer.length < end + 4 + length) { return; }
                const message: unknown = JSON.parse(this.buffer.subarray(end + 4, end + 4 + length).toString('utf8'));
                this.buffer = this.buffer.subarray(end + 4 + length);
                if (!message || typeof message !== 'object' || !('jsonrpc' in message) || message.jsonrpc !== '2.0') { throw new Error('Invalid JSON-RPC response'); }
                const rpc = message as Record<string, unknown>;
                if (typeof rpc.id === 'number') {
                    const pending = this.pending.get(rpc.id); if (!pending) { continue; }
                    this.pending.delete(rpc.id); clearTimeout(pending.timer);
                    if (rpc.error && typeof rpc.error === 'object') { pending.reject(new Error(String((rpc.error as Record<string, unknown>).message))); }
                    else if ('result' in rpc) { pending.resolve(rpc.result); }
                    else { pending.reject(new Error('Missing RPC result')); }
                } else if (typeof rpc.method === 'string') { this.onNotification?.(rpc.method,rpc.params); }
            }
        } catch (error) { this.fail(error instanceof Error ? error : new Error(String(error))); this.child.kill(); }
    }
    private fail(error: Error): void {
        if (this.stopped) { return; } this.stopped = true;
        for (const pending of this.pending.values()) { clearTimeout(pending.timer); pending.reject(error); }
        this.pending.clear(); this.onExit?.(error);
    }
    dispose(): void { this.fail(new Error('Клиент закрыт')); this.child.kill(); }
}
