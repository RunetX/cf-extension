export interface ModelState { loaded: boolean; sourcePath: string | null; dirty: boolean; revision: number }
export interface ModelNode { id: string; label: string; kind: string; hasChildren: boolean; parentId?: string }
export interface InitializeResult { protocolVersion: number; capabilities: Record<string, boolean> }
