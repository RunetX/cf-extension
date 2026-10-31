export interface ModelState {
    loaded: boolean;
    sourcePath: string | null;
    dirty: boolean;
    revision: number;
}
export interface ModelNode {
    id: string;
    label: string;
    kind: string;
    hasChildren: boolean;
    parentId?: string;
    metadataKind?: string;
    typeLabel?: string;
}
export interface ModelProperty {
    name: string;
    label: string;
    kind: 'string' | 'boolean' | 'number' | 'enum' | 'readonly';
    value: unknown;
    editable: boolean;
    options?: string[];
}
export interface PropertyResult {
    id: string;
    properties: ModelProperty[];
}
export interface InitializeResult {
    protocolVersion: number;
    capabilities: Record<string, boolean>;
}
