mod bsl;
mod fixture;
mod model;
mod tree;
use model::Model;
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Read, Write},
    path::Path,
};
const LIMIT: usize = 16 * 1024 * 1024;
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
        if success
            && [
                "model.open",
                "model.setProperty",
                "model.setModule",
                "model.save",
                "model.close",
            ]
            .contains(&method)
        {
            send(
                &mut output,
                json!({"jsonrpc":"2.0","method":"model.changed","params":model.state()}),
            )?;
        }
        for params in model.take_notes() {
            send(
                &mut output,
                json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":params}),
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
