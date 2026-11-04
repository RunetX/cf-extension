# Model RPC v1

JSON-RPC 2.0 stdio: `Content-Length` in UTF-8 bytes, CRLF CRLF, JSON body.
Maximum frame 16 MiB, headers 8192 bytes. EOF exits normally. Invalid framing exits
with stderr; stdout contains framed protocol only. Notifications receive no response.

- `initialize {}` → `{protocolVersion:1,capabilities:{open,tree,properties,edit,save,search,references,composition,modules},positionEncoding:'utf-16',textDocumentSync:1}` (capabilities boolean, LSP subset values).
- `textDocument/didOpen {uri:string,languageId:string,version:number,text:string,moduleId:string,sessionId:number}` (notification) → `textDocument/publishDiagnostics` notification; live BSL diagnostics of the open buffer.
- `textDocument/didChange {uri:string,version:number,text:string}` (notification) → `textDocument/publishDiagnostics`; full sync, version strictly forward.
- `textDocument/didClose {uri:string}` (notification) → `textDocument/publishDiagnostics` with an empty list.
- `model.open {path:string,discardChanges?:boolean}` → State; binary or root XML/directory.
- `model.tree {parentId?:string}` → Node[]; lazy presentation children. One configuration root, Common and class groups, typed child collections (including empty collections), module nodes of existing modules.
- `model.node {id:string}` → Node; direct parent lookup for reveal/search without recursively enumerating the whole tree. Module ids are `module:<entry>`.
- `model.properties {id:string}` → `{id:string,properties:Property[]}`.
- `model.setProperty {id:string,name:string,value:JSON,revision:number}` → State.
- `model.modules {ownerId?:string}` → ModuleInfo[]; existing readable modules, optionally of one owner. Unknown ownerId is an error.
- `model.module {id:string,sessionId:number}` → ModuleDocument; exact library entry only.
- `model.setModule {id:string,sessionId:number,version:number,text:string}` → `{module:ModuleDocument,state:State}`; staged library write must round-trip or the model stays unchanged.
- `model.search {query:string}` → Node[]; case-insensitive names/synonyms, blank yields [].
- `model.references {id:string}` → Node[]; GUID occurrences via the library, no semantic BSL claims.
- `model.composition {id:string}` → Node[]; owned direct children.
- `model.save {path?:string,format?:'binary'|'xml',revision:number}` → State.
- `model.close {discardChanges?:boolean}` → State.
- `shutdown {}` → null, flush then exit.
- `model.changed` notification params = State after successful open/setProperty/setModule/save/close.
`textDocument/publishDiagnostics` params = `{uri:string,version:number,diagnostics:[{range,severity,source,message}]}`; ranges are 0-based UTF-16 (`positionEncoding`), severity 1 = Error. The analysis session is bound to the model: `model.open`/`model.close` clear all documents (a clearing publish is sent for each) and didOpen rejects a foreign `sessionId` with `-32002`. Diagnostics come only from the LSP notifications — the editor never reads them back through the model API, and analysis text never writes into Config (module save stays `model.setModule`).

State = `{loaded:boolean,sourcePath:string|null,dirty:boolean,revision:number,sessionId:number}`.
`sessionId` is a monotonically increasing id of the last successful open (0 before the first one); it is not reused after close. `revision` remains the common model version and does not change on selection.
ModuleInfo = `{id:string,ownerId:string,role:string,label:string}`; `id` is the exact `Config.module_entries` key.
ModuleDocument = ModuleInfo & `{text:string,version:number,sessionId:number}`; `version` is the monotonic per-module version in the current session, initially 0.
Node = `{id:string,label:string,kind:string,hasChildren:boolean,parentId?:string,metadataKind?:string,typeLabel?:string}`; module nodes have `kind:'module'`, the owner as `parentId` and the module role as `metadataKind`.
Property = `{name:string,label:string,kind:'string'|'boolean'|'number'|'enum'|'readonly',value:JSON,editable:boolean,options?:string[]}`.

Object ids use stable library UUIDs; class groups use `group:<class>`, Common uses `common`, and owned collections use `children:<owner UUID>:<kind>`. Each parentId names the actual presentation parent; search/references/composition use these same ids. Virtual groups have no editable properties. Unmapped reference
records use `record:<entry>` ids and are displayed without an editable palette.
Name/Synonym/Comment on Configuration/Catalog/Attribute and Catalog.Hierarchical
are supported. Synonym edits Russian while retaining other languages. Complex or
conditionally unavailable properties are read-only; unknown ids/properties fail.

Revision is monotonically increasing for successful open/edit/save/close. Stale or
invalid edits, failed open and failed saves preserve previous model/state. Binary
save uses a unique create_new sibling temp with PID/counter, sync and atomic rename.
XML export stages into its own temporary directory, requires a new/empty destination
and rejects incomplete reports. XML source save needs an explicit destination.
Dirty open/close requires explicit `discardChanges:true`.

Module writes are session- and version-guarded: a foreign `sessionId`, an exact
`version` mismatch, or an id outside the library module list are `-32002`/`-32602`
errors that keep Config, versions and dirty unchanged. An independent property edit
does not age module versions. The write is staged on a working Config copy and only
published after the library round-trips the exact text. The editor side keeps module
buffers in a `cf-module` virtual FileSystemProvider: URIs carry a random connection
identity and the session id, so tabs of a previous session or a dead server can never
write into a new model.

Errors: -32700 JSON, -32600 request, -32601 method, -32602 params/invalid edit,
-32000 no model, -32001 dirty guard, -32002 revision conflict, -32003 file/format.
Request ids are preserved. Errors do not issue model.changed or clear dirty state.
