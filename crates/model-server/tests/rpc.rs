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
    assert_eq!(nodes.len(), 1, "only the configuration is a top-level node");
    let groups = call(
        &mut input,
        &mut output,
        40,
        "model.tree",
        json!({"parentId":nodes[0]["id"]}),
    );
    let group = groups["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "group:Catalog")
        .unwrap();
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

struct Session {
    child: std::process::Child,
    input: std::process::ChildStdin,
    output: BufReader<std::process::ChildStdout>,
    id: u64,
}
impl Session {
    fn new() -> Self {
        let mut child = server_command()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
            id: 0,
        }
    }
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let body =
            json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params}).to_string();
        write!(self.input, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        self.input.flush().unwrap();
        loop {
            let mut line = String::new();
            assert!(self.output.read_line(&mut line).unwrap() > 0);
            let n: usize = line
                .trim()
                .strip_prefix("Content-Length: ")
                .unwrap()
                .parse()
                .unwrap();
            line.clear();
            self.output.read_line(&mut line).unwrap();
            let mut body = vec![0; n];
            self.output.read_exact(&mut body).unwrap();
            let response: Value = serde_json::from_slice(&body).unwrap();
            if response.get("id").is_some() {
                return response;
            }
        }
    }
    fn ok(&mut self, method: &str, params: Value) -> Value {
        let r = self.call(method, params);
        assert!(r.get("error").is_none(), "{method}: {r}");
        r["result"].clone()
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[test]
fn navigator_edit_transaction_save_and_xml() {
    let dir = std::env::temp_dir().join(format!("cf-extension-nav-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fixture = dir.join("synthetic.cf");
    assert!(server_command()
        .args(["--generate-fixture", fixture.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    // Add a second catalog through the public library API: a real duplicate-name
    // candidate and a real descriptor reference, not a fabricated DTO/brace record.
    let mut cfg = cf_unwrapper::meta::Config::open(&std::fs::read(&fixture).unwrap()).unwrap();
    let mut names = cf_unwrapper::xml::edit::Names::new(&cfg);
    let (warehouse, changes) = cf_unwrapper::xml::edit::add(&cfg, &mut names, "Catalog").unwrap();
    cf_unwrapper::xml::edit::apply(&mut cfg, &changes, false).unwrap();
    names = cf_unwrapper::xml::edit::Names::new(&cfg);
    cf_unwrapper::xml::edit::rename(&mut cfg, &mut names, &warehouse, "Warehouses").unwrap();
    let product = cfg
        .groups
        .iter()
        .flat_map(|g| &g.items)
        .find(|i| i.header.name == "Products")
        .unwrap()
        .header
        .id
        .clone();
    names = cf_unwrapper::xml::edit::Names::new(&cfg);
    let obj = cf_unwrapper::xml::edit::find(&cfg, &product).unwrap();
    let xml = cf_unwrapper::xml::edit::export(&cfg, &names, &obj).unwrap();
    let props = cf_unwrapper::xml::edit::props(&xml, "Catalog").unwrap();
    let xml = cf_unwrapper::xml::edit::set(
        &xml,
        props.iter().find(|p| p.tag == "Synonym").unwrap(),
        &cf_unwrapper::xml::edit::Value::Ml(vec![
            ("en".into(), "English preserved".into()),
            ("ru".into(), "Исходное".into()),
        ]),
    );
    let record = cf_unwrapper::xml::edit::import(&xml, &names, &obj).unwrap();
    let entry = obj.entry.clone();
    cf_unwrapper::xml::edit::store(&mut cfg, &entry, &record).unwrap();
    let obj = cf_unwrapper::xml::edit::find(&cfg, &warehouse).unwrap();
    let xml = cf_unwrapper::xml::edit::export(&cfg, &names, &obj).unwrap();
    let props = cf_unwrapper::xml::edit::props(&xml, "Catalog").unwrap();
    let xml = cf_unwrapper::xml::edit::set(
        &xml,
        props.iter().find(|p| p.tag == "Owners").unwrap(),
        &cf_unwrapper::xml::edit::Value::List(vec!["Catalog.Products".into()]),
    );
    let record = cf_unwrapper::xml::edit::import(&xml, &names, &obj).unwrap();
    let entry = obj.entry.clone();
    cf_unwrapper::xml::edit::store(&mut cfg, &entry, &record).unwrap();
    std::fs::write(&fixture, cfg.save()).unwrap();
    let mut s = Session::new();
    let mut state = s.ok("model.open", json!({"path":fixture}));
    let roots = s.ok("model.tree", json!({}));
    let root = roots[0]["id"].clone();
    let objects = s.ok("model.tree", json!({"parentId":"group:Catalog"}));
    let catalog = objects
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["label"] == "Products")
        .unwrap()["id"]
        .clone();
    let children = s.ok("model.composition", json!({"id":catalog}));
    let attribute = children
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "Attribute")
        .unwrap()["id"]
        .clone();
    for id in [&root, &catalog, &attribute] {
        let props = s.ok("model.properties", json!({"id":id}));
        for name in ["Name", "Synonym", "Comment"] {
            assert!(
                props["properties"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["name"] == name && p["editable"] == true),
                "{id}: {name}: {props}"
            );
        }
    }
    for (id, name, value) in [
        (&root, "Name", json!("RootChanged")),
        (&root, "Synonym", json!("Корень")),
        (&root, "Comment", json!("Root comment")),
        (&catalog, "Name", json!("RenamedProducts")),
        (&catalog, "Synonym", json!("Товары")),
        (&catalog, "Comment", json!("<script>bad</script>")),
        (&catalog, "Hierarchical", json!(true)),
        (&attribute, "Name", json!("CodeChanged")),
        (&attribute, "Synonym", json!("Код")),
        (&attribute, "Comment", json!("Nested comment")),
    ] {
        state = s.ok(
            "model.setProperty",
            json!({"id":id,"name":name,"value":value,"revision":state["revision"]}),
        );
        assert_eq!(state["dirty"], true);
        let p = s.ok("model.properties", json!({"id":id}));
        assert_eq!(
            p["properties"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["name"] == name)
                .unwrap()["value"],
            value
        );
    }
    let before = s.ok("model.properties", json!({"id":catalog}));
    for (name, value, revision, code) in [
        (
            "Name",
            json!("1 invalid"),
            state["revision"].clone(),
            -32602,
        ),
        (
            "Hierarchical",
            json!("true"),
            state["revision"].clone(),
            -32602,
        ),
        ("Missing", json!(true), state["revision"].clone(), -32602),
        (
            "Name",
            json!("Warehouses"),
            state["revision"].clone(),
            -32602,
        ),
        ("Name", json!("Stale"), json!(1), -32002),
    ] {
        assert_eq!(
            s.call(
                "model.setProperty",
                json!({"id":catalog,"name":name,"value":value,"revision":revision})
            )["error"]["code"],
            code
        );
        assert_eq!(s.ok("model.properties", json!({"id":catalog})), before);
    }
    assert_eq!(s.call("model.close", json!({}))["error"]["code"], -32001);
    assert_eq!(
        s.call("model.open", json!({"path":fixture}))["error"]["code"],
        -32001
    );
    let bytes = std::fs::read(&fixture).unwrap();
    assert_eq!(
        s.call(
            "model.save",
            json!({"path":dir.join("missing/out.cf"),"revision":state["revision"]})
        )["error"]["code"],
        -32003
    );
    assert_eq!(std::fs::read(&fixture).unwrap(), bytes);
    assert!(s
        .ok("model.search", json!({"query":"ТОВАР"}))
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == catalog));
    assert!(!s
        .ok("model.references", json!({"id":catalog}))
        .as_array()
        .unwrap()
        .is_empty());
    assert!(s
        .ok("model.references", json!({"id":catalog}))
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == warehouse));
    state = s.ok("model.save", json!({"revision":state["revision"]}));
    assert_eq!(state["dirty"], false);
    state = s.ok("model.open", json!({"path":fixture}));
    assert_eq!(s.ok("model.properties", json!({"id":catalog})), before);
    let saved_cfg = cf_unwrapper::meta::Config::open(&std::fs::read(&fixture).unwrap()).unwrap();
    let names = cf_unwrapper::xml::edit::Names::new(&saved_cfg);
    let obj = cf_unwrapper::xml::edit::find(&saved_cfg, catalog.as_str().unwrap()).unwrap();
    assert!(cf_unwrapper::xml::edit::export(&saved_cfg, &names, &obj)
        .unwrap()
        .contains("English preserved"));
    for original in cfg
        .entries
        .iter()
        .filter(|e| e.name != cfg.root_id && e.name != product)
    {
        assert_eq!(
            saved_cfg.entries[saved_cfg.index[&original.name]].data, original.data,
            "unaffected entry {}",
            original.name
        );
    }
    let xml = dir.join("xml");
    state = s.ok(
        "model.save",
        json!({"path":xml,"format":"xml","revision":state["revision"]}),
    );
    assert!(xml.join("Configuration.xml").is_file());
    let saved = std::fs::read(xml.join("Configuration.xml")).unwrap();
    assert_eq!(
        s.call(
            "model.save",
            json!({"path":xml,"format":"xml","revision":state["revision"]})
        )["error"]["code"],
        -32003
    );
    assert_eq!(std::fs::read(xml.join("Configuration.xml")).unwrap(), saved);
    s.ok("model.open", json!({"path":xml}));
    assert_eq!(s.ok("model.properties", json!({"id":catalog})), before);
    s.ok("model.close", json!({}));
    let epf = dir.join("synthetic.epf");
    std::fs::write(
        &epf,
        cf_unwrapper::xml::imp::new_external("SyntheticProcessor").unwrap(),
    )
    .unwrap();
    state = s.ok("model.open", json!({"path":epf}));
    let external = s.ok("model.tree", json!({}))[0]["id"].clone();
    state=s.ok("model.setProperty",json!({"id":external,"name":"Comment","value":"EPF roundtrip","revision":state["revision"]}));
    s.ok("model.save", json!({"revision":state["revision"]}));
    s.ok("model.open", json!({"path":epf}));
    assert!(
        s.ok("model.properties", json!({"id":external}))["properties"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "Comment" && p["value"] == "EPF roundtrip")
    );
    s.ok("model.close", json!({}));
    s.ok("shutdown", json!({}));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn dependent_property_is_readonly_and_rejected() {
    let dir = std::env::temp_dir().join(format!("cf-extension-dependent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("synthetic.cf");
    assert!(server_command()
        .args(["--generate-fixture", path.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    let mut s = Session::new();
    let state = s.ok("model.open", json!({"path":path}));
    let nodes = s.ok("model.tree", json!({"parentId":"group:Catalog"}));
    let id = nodes[0]["id"].clone();
    let props = s.ok("model.properties", json!({"id":id}));
    assert_eq!(
        props["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "HierarchyType")
            .unwrap()["editable"],
        false,
        "HierarchyType is unavailable while Hierarchical=false"
    );
    assert_eq!(s.call("model.setProperty",json!({"id":id,"name":"HierarchyType","value":"HierarchyOfItems","revision":state["revision"]}))["error"]["code"],-32602);
    assert_eq!(s.ok("model.properties", json!({"id":id})), props);
    s.ok("model.close", json!({}));
    s.ok("shutdown", json!({}));
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn selected_xml_open_preserves_state_on_failure_and_roundtrips_roots() {
    let dir =
        std::env::temp_dir().join(format!("cf-extension-selected-xml-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fixture = dir.join("synthetic.cf");
    assert!(server_command()
        .args(["--generate-fixture", fixture.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    let mut s = Session::new();
    let state = s.ok("model.open", json!({"path":fixture}));
    let xml = dir.join("xml");
    s.ok(
        "model.save",
        json!({"path":xml,"format":"xml","revision":state["revision"]}),
    );
    let malformed = xml.join("Malformed.xml");
    let unrelated = xml.join("Unrelated.xml");
    let dump_info = xml.join("ConfigDumpInfo.xml");
    std::fs::write(&dump_info, b"<ConfigDumpInfo/>").unwrap();
    let paths = [
        xml.join("MISSING.xml"),
        malformed,
        unrelated,
        dump_info,
        xml.join("Catalogs/Products.xml"),
    ];
    for mode in ["empty", "clean", "dirty"] {
        for path in &paths {
            let mut before = s.ok("model.close", json!({"discardChanges":true}));
            let mut id = Value::Null;
            if mode != "empty" {
                before = s.ok("model.open", json!({"path":fixture}));
                id = s.ok("model.tree", json!({}))[0]["id"].clone();
                if mode == "dirty" {
                    before = s.ok("model.setProperty", json!({"id":id,"name":"Comment","value":"unsaved XML failure sentinel","revision":before["revision"]}));
                }
            }
            let tree = if mode != "empty" {
                Some(s.ok("model.tree", json!({})))
            } else {
                None
            };
            let props = if mode != "empty" {
                Some(s.ok("model.properties", json!({"id":id})))
            } else {
                None
            };
            if path.ends_with("Malformed.xml") {
                std::fs::write(path, b"<broken").unwrap();
            } else if path.ends_with("Unrelated.xml") {
                std::fs::write(path, b"<Unrelated/>").unwrap();
            }
            let response = s.call("model.open", json!({"path":path,"discardChanges":true}));
            assert_eq!(
                response["error"]["code"],
                -32003,
                "{mode}: {}: {response}",
                path.display()
            );
            if path.ends_with("Malformed.xml") || path.ends_with("Unrelated.xml") {
                std::fs::remove_file(path).unwrap();
            }
            if let Some(tree) = &tree {
                assert_eq!(&s.ok("model.tree", json!({})), tree);
                assert_eq!(s.ok("model.properties", json!({"id":id})), props.unwrap());
                let after = if mode == "dirty" {
                    assert_eq!(s.call("model.close", json!({}))["error"]["code"], -32001);
                    s.ok("model.setProperty", json!({"id":id,"name":"Comment","value":"still unsaved","revision":before["revision"]}))
                } else {
                    s.ok("model.save", json!({"revision":before["revision"]}))
                };
                before["revision"] = json!(before["revision"].as_u64().unwrap() + 1);
                assert_eq!(after, before);
            } else {
                assert_eq!(s.call("model.tree", json!({}))["error"]["code"], -32000);
                let after = s.ok("model.close", json!({}));
                before["revision"] = json!(before["revision"].as_u64().unwrap() + 1);
                assert_eq!(after, before);
            }
        }
    }
    s.ok(
        "model.open",
        json!({"path":xml.join("Configuration.xml"),"discardChanges":true}),
    );
    assert_eq!(
        s.ok("model.tree", json!({}))[0]["label"],
        "NavigatorSynthetic"
    );
    let epf = dir.join("synthetic.epf");
    std::fs::write(
        &epf,
        cf_unwrapper::xml::imp::new_external("SyntheticProcessor").unwrap(),
    )
    .unwrap();
    s.ok("model.open", json!({"path":epf}));
    let external = dir.join("external");
    let cfg = cf_unwrapper::meta::Config::open(&std::fs::read(&epf).unwrap()).unwrap();
    let report = cf_unwrapper::xml::export(&cfg, &external).unwrap();
    assert!(report.errors.is_empty());
    assert!(report.skipped.is_empty());
    let root = external.join("SyntheticProcessor.xml");
    assert!(root.is_file());
    s.ok("model.open", json!({"path":root}));
    assert_eq!(
        s.ok("model.tree", json!({}))[0]["label"],
        "SyntheticProcessor"
    );
    s.ok("shutdown", json!({}));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn external_processor_xml_save_roundtrip() {
    let dir =
        std::env::temp_dir().join(format!("cf-extension-epf-xml-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let epf = dir.join("synthetic.epf");
    std::fs::write(
        &epf,
        cf_unwrapper::xml::imp::new_external("XmlSaveProcessor").unwrap(),
    )
    .unwrap();
    let mut s = Session::new();
    let state = s.ok("model.open", json!({"path":epf}));
    let id = s.ok("model.tree", json!({}))[0]["id"].clone();
    let state = s.ok("model.setProperty", json!({"id":id,"name":"Comment","value":"EPF XML save roundtrip","revision":state["revision"]}));
    let expected = s.ok("model.properties", json!({"id":id}));
    let xml = dir.join("xml");
    let saved = s.ok(
        "model.save",
        json!({"path":xml,"format":"xml","revision":state["revision"]}),
    );
    assert_eq!(saved["dirty"], false);
    assert_eq!(saved["sourcePath"], json!(xml));
    assert_eq!(
        saved["revision"].as_u64().unwrap(),
        state["revision"].as_u64().unwrap() + 1
    );
    assert!(!xml.join("Configuration.xml").exists());
    let root = xml.join("XmlSaveProcessor.xml");
    assert!(root.is_file());
    s.ok("model.open", json!({"path":root}));
    assert_eq!(s.ok("model.properties", json!({"id":id})), expected);
    let tree = s.ok("model.tree", json!({}));
    assert_eq!(tree[0]["label"], "XmlSaveProcessor");
    s.ok("shutdown", json!({}));
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn configurator_hierarchy_and_parent_navigation() {
    let dir = std::env::temp_dir().join(format!("cf-extension-hierarchy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("synthetic.cf");
    assert!(server_command()
        .args(["--generate-fixture", file.to_str().unwrap()])
        .output()
        .unwrap()
        .status
        .success());
    let mut s = Session::new();
    s.ok("model.open", json!({"path":file}));
    let roots = s.ok("model.tree", json!({}));
    assert_eq!(roots.as_array().unwrap().len(), 1);
    let root = &roots[0];
    assert_eq!(root["kind"], "Configuration");
    assert_eq!(root["hasChildren"], true);
    let groups = s.ok("model.tree", json!({"parentId":root["id"]}));
    assert_eq!(groups[0]["label"], "Общие");
    assert_eq!(groups[0]["parentId"], root["id"]);
    assert!(groups
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == "group:Constant" && n["hasChildren"] == false));
    let common = s.ok("model.tree", json!({"parentId":"common"}));
    assert_eq!(common[0]["label"], "Подсистемы");
    assert!(common
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == "group:CommonModule"));
    assert!(!groups
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == "group:CommonModule"));
    let documents = s.ok("model.tree", json!({"parentId":"group:Document"}));
    assert!(documents
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == "group:DocumentNumerator"));
    let catalogs = s.ok("model.tree", json!({"parentId":"group:Catalog"}));
    let catalog = catalogs
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["label"] == "Products")
        .unwrap();
    let collections = s.ok("model.tree", json!({"parentId":catalog["id"]}));
    let labels: Vec<_> = collections
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["label"].as_str().unwrap())
        .collect();
    assert_eq!(
        labels,
        vec!["Реквизиты", "Табличные части", "Формы", "Команды", "Макеты"]
    );
    let attrs = s.ok("model.tree", json!({"parentId":collections[0]["id"]}));
    assert_eq!(attrs.as_array().unwrap().len(), 1);
    assert_eq!(attrs[0]["parentId"], collections[0]["id"]);
    assert_eq!(attrs[0]["metadataKind"], "Attribute");
    let lookup = s.ok("model.node", json!({"id":collections[0]["id"]}));
    assert_eq!(lookup, collections[0]);
    assert_eq!(lookup["parentId"], catalog["id"]);
    assert_eq!(
        s.ok("model.node", json!({"id":"group:Catalog"}))["parentId"],
        root["id"]
    );
    let forms = collections
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["label"] == "Формы")
        .unwrap();
    assert_eq!(forms["hasChildren"], false);
    assert!(s
        .ok("model.tree", json!({"parentId":forms["id"]}))
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        s.call("model.node", json!({"id":"group:NotAClass"}))["error"]["code"],
        -32602
    );
    let external = dir.join("external.epf");
    std::fs::write(
        &external,
        cf_unwrapper::xml::imp::new_external("ExternalTree").unwrap(),
    )
    .unwrap();
    s.ok("model.open", json!({"path":external}));
    let external_root = s.ok("model.tree", json!({}));
    assert_eq!(external_root.as_array().unwrap().len(), 1);
    let children = s.ok("model.tree", json!({"parentId":external_root[0]["id"]}));
    assert_eq!(children[0]["label"], "Реквизиты");
    assert_eq!(children.as_array().unwrap().len(), 4);
    assert_eq!(
        s.call("model.node", json!({"id":"common"}))["error"]["code"],
        -32602
    );
    drop(s);
    let canonical = dir.canonicalize().unwrap();
    assert!(canonical.starts_with(std::env::temp_dir().canonicalize().unwrap()));
    std::fs::remove_dir_all(canonical).unwrap();
}
