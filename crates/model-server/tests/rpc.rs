use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
fn server_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_model-server"));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}
#[test]
fn rpc_scaffold() {
    let mut child = server_command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    fn call(
        input: &mut impl Write,
        output: &mut (impl BufRead + Read),
        id: i32,
        method: &str,
        params: Value,
    ) -> Value {
        let body = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string();
        write!(input, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        input.flush().unwrap();
        loop {
            let mut line = String::new();
            assert!(
                output.read_line(&mut line).unwrap() > 0,
                "server exited before RPC response"
            );
            let length: usize = line
                .trim()
                .strip_prefix("Content-Length: ")
                .unwrap()
                .parse()
                .unwrap();
            line.clear();
            output.read_line(&mut line).unwrap();
            assert_eq!(line, "\r\n");
            let mut body = vec![0; length];
            output.read_exact(&mut body).unwrap();
            let response: Value = serde_json::from_slice(&body).unwrap();
            if response.get("id").is_some() {
                assert_eq!(response["id"], id);
                return response;
            }
        }
    }
    assert_eq!(
        call(&mut input, &mut output, 1, "initialize", json!({}))["result"]["protocolVersion"],
        1
    );
    assert_eq!(
        call(&mut input, &mut output, 2, "unknown", json!({}))["error"]["code"],
        -32601
    );
    let dir = std::env::temp_dir().join(format!("cf-extension-rpc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fixture = dir.join("synthetic.cf");
    let generation = server_command()
        .arg("--generate-fixture")
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(
        generation.status.success(),
        "{}",
        String::from_utf8_lossy(&generation.stderr)
    );
    let opened = call(
        &mut input,
        &mut output,
        3,
        "model.open",
        json!({"path":fixture}),
    );
    assert_eq!(opened["result"]["loaded"], true);
    assert_eq!(opened["result"]["revision"], 1);
    let tree = call(&mut input, &mut output, 4, "model.tree", json!({}));
    let nodes = tree["result"].as_array().unwrap();
    assert!(nodes.iter().any(|n| n["label"] == "NavigatorSynthetic"));
    let group = nodes.iter().find(|n| n["kind"] == "group").unwrap();
    let objects = call(
        &mut input,
        &mut output,
        5,
        "model.tree",
        json!({"parentId":group["id"]}),
    );
    assert!(objects["result"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["label"] == "Products"));
    let bad = dir.join("bad.cf");
    std::fs::write(&bad, b"broken").unwrap();
    assert_eq!(
        call(
            &mut input,
            &mut output,
            6,
            "model.open",
            json!({"path":bad})
        )["error"]["code"],
        -32003
    );
    assert_eq!(
        call(&mut input, &mut output, 7, "model.tree", json!({}))["result"],
        tree["result"]
    );
    assert_eq!(
        call(&mut input, &mut output, 8, "model.open", json!({}))["error"]["code"],
        -32602
    );
    call(&mut input, &mut output, 9, "shutdown", json!({}));
    drop(input);
    assert!(child.wait().unwrap().success());
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn rpc_rejects_invalid_requests_and_params() {
    let bodies = [
        "{broken".to_string(),
        json!({"jsonrpc":"2.0","id":true,"method":"initialize","params":{}}).to_string(),
        json!({"jsonrpc":"2.0","id":"params","method":"initialize","params":[]}).to_string(),
        json!({"jsonrpc":"2.0","id":"guard","method":"model.open","params":{"path":"missing.cf","discardChanges":"yes"}}).to_string(),
        json!({"jsonrpc":"2.0","method":"initialize","params":{}}).to_string(),
        json!({"jsonrpc":"2.0","id":5,"method":"shutdown","params":{}}).to_string(),
    ];
    let mut child = server_command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for body in bodies {
        write!(input, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let mut reader = BufReader::new(output.stdout.as_slice());
    let mut responses = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        let length: usize = line
            .trim()
            .strip_prefix("Content-Length: ")
            .unwrap()
            .parse()
            .unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        responses.push(serde_json::from_slice::<Value>(&body).unwrap());
    }
    assert_eq!(
        responses.len(),
        5,
        "notifications must not receive responses"
    );
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1]["error"]["code"], -32600);
    assert_eq!(responses[2]["error"]["code"], -32602);
    assert_eq!(responses[3]["error"]["code"], -32602);
    assert_eq!(responses[3]["id"], "guard");
}
