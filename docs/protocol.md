# Scaffold RPC v1

Frames: Content-Length (UTF-8 byte count), CRLF CRLF, JSON-RPC 2.0 object. Max 16 MiB,
header max 8192 bytes. EOF closes normally; malformed framing exits with stderr.

- initialize {} → {protocolVersion:1,capabilities:{open:true,tree:true}}
- model.open {path:string,discardChanges?:boolean} → State
- model.tree {parentId?:string} → Node[]
- shutdown {} → null, flush then exit
- notification model.changed: State after open

State = {loaded:boolean,sourcePath:string|null,dirty:boolean,revision:number}.
Node = {id:string,label:string,kind:string,hasChildren:boolean,parentId?:string}.
Root metadata id is the library Item header UUID; groups use group:<class> ids.
Root tree returns the configuration and nonempty metadata groups. Children are lazy.

Errors: -32700 JSON, -32600 request, -32601 method, -32602 params, -32000 no model,
-32003 file/format. Request ids survive unchanged; notifications have no response.
Opening increments revision, and a rejected open preserves the existing state.
Dirty editing guards and remaining protocol methods belong to the navigator increment.
