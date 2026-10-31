mod fixture;
use cf_unwrapper::{
    meta::{Config, Item},
    xml,
};
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Read, Write},
    path::Path,
};
const LIMIT: usize = 16 * 1024 * 1024;
#[derive(Default)]
struct Model {
    config: Option<Config>,
    source: Option<String>,
    revision: u64,
}
impl Model {
    fn state(&self) -> Value {
        json!({"loaded":self.config.is_some(),"sourcePath":self.source,"dirty":false,"revision":self.revision})
    }
    fn call(&mut self, method: &str, params: &Value) -> Result<Value, (i32, String)> {
        if !params.is_object() {
            return Err((-32602, "params must be an object".into()));
        }
        match method {
            "initialize" => {
                Ok(json!({"protocolVersion":1,"capabilities":{"open":true,"tree":true}}))
            }
            "shutdown" => Ok(Value::Null),
            "model.open" => {
                if params
                    .get("discardChanges")
                    .is_some_and(|v| !v.is_boolean())
                {
                    return Err((-32602, "discardChanges must be boolean".into()));
                }
                let path = params
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "path must be a string".into()))?;
                let config = open(Path::new(path)).map_err(|e| (-32003, e))?;
                self.config = Some(config);
                self.source = Some(path.into());
                self.revision += 1;
                Ok(self.state())
            }
            "model.tree" => {
                let cfg = self
                    .config
                    .as_ref()
                    .ok_or((-32000, "Откройте конфигурацию".into()))?;
                if params.get("parentId").is_some_and(|v| !v.is_string()) {
                    return Err((-32602, "parentId must be a string".into()));
                }
                let parent = params.get("parentId").and_then(Value::as_str);
                let nodes = match parent {
                    None => {
                        let mut nodes = vec![node(&cfg.root, None)];
                        nodes.extend(cfg.groups.iter().map(|g|json!({"id":format!("group:{}",g.class),"label":g.title,"kind":"group","hasChildren":!g.items.is_empty()})));
                        nodes
                    }
                    Some(id) if id.starts_with("group:") => cfg
                        .groups
                        .iter()
                        .find(|g| Some(g.class) == id.strip_prefix("group:"))
                        .map(|g| g.items.iter().map(|i| node(i, Some(id))).collect())
                        .ok_or((-32602, "Unknown group".into()))?,
                    Some(id) => find(&cfg.root, id)
                        .or_else(|| {
                            cfg.groups
                                .iter()
                                .flat_map(|g| &g.items)
                                .find_map(|i| find(i, id))
                        })
                        .map(|i| i.children.iter().map(|c| node(c, Some(id))).collect())
                        .ok_or((-32602, "Unknown node".into()))?,
                };
                Ok(json!(nodes))
            }
            _ => Err((-32601, format!("Unknown method: {method}"))),
        }
    }
}
fn node(item: &Item, parent: Option<&str>) -> Value {
    let mut n = json!({"id":item.header.id,"label":item.header.name,"kind":item.kind,"hasChildren":!item.children.is_empty()});
    if let Some(p) = parent {
        n["parentId"] = json!(p);
    }
    n
}
fn find<'a>(item: &'a Item, id: &str) -> Option<&'a Item> {
    if item.header.id == id {
        Some(item)
    } else {
        item.children.iter().find_map(|i| find(i, id))
    }
}
fn open(path: &Path) -> Result<Config, String> {
    if path.is_dir()
        || path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("xml"))
    {
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().ok_or("XML has no parent")?
        };
        let imported = xml::imp::import(dir, None)?;
        if !imported.refused.is_empty() {
            return Err(format!("XML import refused: {:?}", imported.refused));
        }
        Config::open(&xml::imp::cf(dir, &imported)?)
    } else {
        Config::open(&std::fs::read(path).map_err(|e| e.to_string())?)
    }
}
fn frame(input: &mut impl BufRead) -> Result<Option<Vec<u8>>, String> {
    let mut size = None;
    let mut total = 0;
    loop {
        let mut bytes = Vec::new();
        let count = input
            .take(8193)
            .read_until(b'\n', &mut bytes)
            .map_err(|e| e.to_string())?;
        if count == 0 {
            return if total == 0 {
                Ok(None)
            } else {
                Err("Unexpected EOF in frame header".into())
            };
        }
        total += count;
        if total > 8192 {
            return Err("Frame header exceeds 8192 bytes".into());
        }
        if bytes == b"\r\n" {
            break;
        }
        let line = std::str::from_utf8(&bytes).map_err(|_| "Invalid frame header")?;
        let (name, value) = line
            .trim_end()
            .split_once(':')
            .ok_or("Malformed frame header")?;
        if name.eq_ignore_ascii_case("Content-Length") {
            if size.is_some() {
                return Err("Duplicate Content-Length".into());
            }
            size = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "Invalid Content-Length")?,
            );
        }
    }
    let size = size.ok_or("Missing Content-Length")?;
    if size > LIMIT {
        return Err("Frame exceeds 16 MiB".into());
    }
    let mut body = vec![0; size];
    input.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok(Some(body))
}
fn send(output: &mut impl Write, body: Value) -> Result<(), String> {
    let body = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    write!(output, "Content-Length: {}\r\n\r\n", body.len()).map_err(|e| e.to_string())?;
    output
        .write_all(&body)
        .and_then(|_| output.flush())
        .map_err(|e| e.to_string())
}
fn serve() -> Result<(), String> {
    let mut model = Model::default();
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    while let Some(body) = frame(&mut input)? {
        let request: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => {
                send(
                    &mut output,
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}}),
                )?;
                continue;
            }
        };
        let valid = request.is_object()
            && request["jsonrpc"] == "2.0"
            && request["method"].is_string()
            && request
                .get("id")
                .is_none_or(|id| id.is_null() || id.is_string() || id.is_number());
        if !valid {
            send(
                &mut output,
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid request"}}),
            )?;
            continue;
        }
        let method = request["method"].as_str().unwrap();
        let result = model.call(method, request.get("params").unwrap_or(&json!({})));
        let success = result.is_ok();
        if let Some(id) = request.get("id") {
            let response = match result {
                Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
                Err((code, message)) => {
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
                }
            };
            send(&mut output, response)?;
        }
        if success && method == "model.open" {
            send(
                &mut output,
                json!({"jsonrpc":"2.0","method":"model.changed","params":model.state()}),
            )?;
        }
        if success && method == "shutdown" {
            break;
        }
    }
    Ok(())
}
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let result = if args.len() == 3 && args[1] == "--generate-fixture" {
        fixture::generate(Path::new(&args[2]))
    } else {
        serve()
    };
    if let Err(e) = result {
        eprintln!("model-server: {e}");
        std::process::exit(1);
    }
}
