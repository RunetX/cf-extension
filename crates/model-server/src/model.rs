use cf_unwrapper::{
    meta::{Config, Item},
    xml,
};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
type Error = (i32, String);
type Result<T> = std::result::Result<T, Error>;
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Default)]
pub struct Model {
    config: Option<Config>,
    source: Option<String>,
    revision: u64,
    session_id: u64,
    module_versions: std::collections::HashMap<String, u64>,
    dirty: bool,
    xml_source: bool,
    analysis: crate::bsl::analysis::Session,
    notes: Vec<Value>,
}
fn bad(message: impl Into<String>) -> Error {
    (-32602, message.into())
}
fn format_error(message: impl ToString) -> Error {
    (-32003, message.to_string())
}
fn text<'a>(params: &'a Value, name: &str) -> Result<&'a str> {
    params
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("{name}: требуется строка")))
}
impl Model {
    pub fn state(&self) -> Value {
        json!({"loaded":self.config.is_some(),"sourcePath":self.source,"dirty":self.dirty,"revision":self.revision,"sessionId":self.session_id})
    }
    fn cfg(&self) -> Result<&Config> {
        self.config
            .as_ref()
            .ok_or((-32000, "Откройте конфигурацию".into()))
    }
    /// Исходящие уведомления (textDocument/publishDiagnostics): забирает цикл serve после ответа.
    pub fn take_notes(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.notes)
    }
    fn clear_analysis(&mut self) {
        for params in self.analysis.clear() {
            self.notes.push(params);
        }
    }
    /// Роль модуля по точному ключу записи (для выбора места компиляции в анализе).
    fn module_role(&self, id: &str) -> Result<String> {
        let cfg = self.cfg()?;
        modules(cfg)
            .into_iter()
            .find(|m| m["id"] == id)
            .map(|m| m["role"].as_str().unwrap_or_default().to_string())
            .ok_or_else(|| bad("Неизвестный или недоступный модуль"))
    }
    fn guard(&self, p: &Value) -> Result<()> {
        if p.get("discardChanges").is_some_and(|v| !v.is_boolean()) {
            return Err(bad("discardChanges must be boolean"));
        }
        if self.dirty && p.get("discardChanges") != Some(&Value::Bool(true)) {
            return Err((
                -32001,
                "Есть несохранённые изменения. Сохраните или явно отбросьте их.".into(),
            ));
        }
        Ok(())
    }
    fn revision(&self, p: &Value) -> Result<()> {
        let r = p
            .get("revision")
            .and_then(Value::as_u64)
            .ok_or_else(|| bad("revision: требуется целое число"))?;
        if r != self.revision {
            return Err((-32002, "Модель изменилась. Обновите палитру.".into()));
        }
        Ok(())
    }
    fn module(&self, p: &Value) -> Result<Value> {
        let session = p
            .get("sessionId")
            .and_then(Value::as_u64)
            .ok_or_else(|| bad("sessionId: требуется целое число"))?;
        if session != self.session_id {
            return Err((
                -32002,
                "Конфигурация сменилась; документ старой сессии".into(),
            ));
        }
        let cfg = self.cfg()?;
        let id = text(p, "id")?;
        let mut info = modules(cfg)
            .into_iter()
            .find(|m| m["id"] == id)
            .ok_or_else(|| bad("Неизвестный или недоступный модуль"))?;
        info["text"] = json!(cfg
            .module_text(id)
            .ok_or_else(|| format_error("Модуль не читается"))?);
        info["version"] = json!(self.module_versions.get(id).copied().unwrap_or(0));
        info["sessionId"] = json!(self.session_id);
        Ok(info)
    }
    pub fn call(&mut self, method: &str, p: &Value) -> Result<Value> {
        if !p.is_object() {
            return Err(bad("params must be an object"));
        }
        match method {
            "initialize" => Ok(
                json!({"protocolVersion":1,"capabilities":{"open":true,"tree":true,"node":true,"properties":true,"edit":true,"save":true,"search":true,"references":true,"composition":true,"modules":true},"positionEncoding":"utf-16","textDocumentSync":1}),
            ),
            "shutdown" => Ok(Value::Null),
            "textDocument/didOpen" => {
                if self.config.is_none() {
                    return Err((-32000, "Откройте конфигурацию".into()));
                }
                let session = p
                    .get("sessionId")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| bad("sessionId: требуется целое число"))?;
                if session != self.session_id {
                    return Err((
                        -32002,
                        "Конфигурация сменилась; документ старой сессии".into(),
                    ));
                }
                let id = text(p, "moduleId")?;
                let role = self.module_role(id)?;
                let uri = text(p, "uri")?;
                let version = p
                    .get("version")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| bad("version: требуется целое число"))?;
                let value = text(p, "text")?;
                if self.analysis.module_of(uri).is_some() {
                    return Err(bad("Документ анализа уже открыт"));
                }
                let params = self.analysis.open(
                    uri,
                    id,
                    version,
                    value,
                    self.config.as_ref().unwrap(),
                    &role,
                );
                self.notes.push(params);
                Ok(Value::Null)
            }
            "textDocument/didChange" => {
                if self.config.is_none() {
                    return Err((-32000, "Откройте конфигурацию".into()));
                }
                let uri = text(p, "uri")?;
                let version = p
                    .get("version")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| bad("version: требуется целое число"))?;
                let value = text(p, "text")?;
                let role = match self.analysis.module_of(uri) {
                    Some(id) => self.module_role(id)?,
                    None => return Err(bad("Неизвестный документ анализа")),
                };
                let params = self.analysis.change(
                    uri,
                    version,
                    value,
                    self.config.as_ref().unwrap(),
                    &role,
                )?;
                self.notes.push(params);
                Ok(Value::Null)
            }
            "textDocument/didClose" => {
                let uri = text(p, "uri")?;
                if let Some(params) = self.analysis.close(uri) {
                    self.notes.push(params);
                }
                Ok(Value::Null)
            }
            "model.open" => {
                self.guard(p)?;
                let path = text(p, "path")?;
                let cfg = open(Path::new(path)).map_err(format_error)?;
                self.session_id += 1;
                self.module_versions.clear();
                self.xml_source = is_xml(Path::new(path));
                self.config = Some(cfg);
                self.source = Some(path.into());
                self.dirty = false;
                self.revision += 1;
                self.clear_analysis();
                Ok(self.state())
            }
            "model.close" => {
                self.guard(p)?;
                self.config = None;
                self.source = None;
                self.dirty = false;
                self.revision += 1;
                self.clear_analysis();
                Ok(self.state())
            }
            "model.modules" => {
                let cfg = self.cfg()?;
                let owner = p
                    .get("ownerId")
                    .map(|v| v.as_str().ok_or_else(|| bad("ownerId: требуется строка")))
                    .transpose()?;
                if let Some(owner) = owner {
                    item(cfg, owner)?;
                }
                Ok(json!(modules(cfg)
                    .into_iter()
                    .filter(|m| owner.is_none_or(|id| m["ownerId"] == id))
                    .collect::<Vec<_>>()))
            }
            "model.module" => self.module(p),
            "model.setModule" => {
                let before = self.module(p)?;
                let version = p
                    .get("version")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| bad("version: требуется целое число"))?;
                if before["version"] != version {
                    return Err((-32002, "Модуль изменился; сохранение отклонено".into()));
                }
                let id = text(p, "id")?;
                let value = text(p, "text")?;
                let mut staged = Config::open(&self.cfg()?.save()).map_err(format_error)?;
                staged.set_module_text(id, value).map_err(format_error)?;
                let checked = Config::open(&staged.save()).map_err(format_error)?;
                let expected = value.replace("\r\n", "\n").replace('\n', "\r\n");
                if checked.module_text(id).as_deref() != Some(expected.as_str()) {
                    return Err(format_error(
                        "Библиотека не сохранила модуль; модель не изменена",
                    ));
                }
                self.config = Some(checked);
                self.module_versions.insert(id.to_string(), version + 1);
                self.dirty = true;
                self.revision += 1;
                Ok(json!({"module":self.module(p)?,"state":self.state()}))
            }
            "model.tree" => {
                let cfg = self.cfg()?;
                if p.get("parentId").is_some_and(|v| !v.is_string()) {
                    return Err(bad("parentId must be a string"));
                }
                Ok(json!(crate::tree::children(
                    cfg,
                    p.get("parentId").and_then(Value::as_str)
                )
                .map_err(bad)?))
            }
            "model.node" => {
                let cfg = self.cfg()?;
                crate::tree::node(cfg, text(p, "id")?).map_err(bad)
            }
            "model.composition" => {
                let cfg = self.cfg()?;
                let id = text(p, "id")?;
                Ok(json!(item(cfg, id)?
                    .children
                    .iter()
                    .map(|i| crate::tree::from_structural(i, Some(id)))
                    .collect::<Vec<_>>()))
            }
            "model.search" => {
                let cfg = self.cfg()?;
                let q = text(p, "query")?.trim().to_lowercase();
                if q.is_empty() {
                    return Ok(json!([]));
                }
                Ok(json!(all_nodes(cfg)
                    .into_iter()
                    .filter(|(i, _)| i.header.name.to_lowercase().contains(&q)
                        || i.header.synonym.to_lowercase().contains(&q))
                    .map(|(i, parent)| crate::tree::from_structural(i, parent.as_deref()))
                    .collect::<Vec<_>>()))
            }
            "model.references" => {
                let cfg = self.cfg()?;
                let id = text(p, "id")?;
                item(cfg, id)?;
                let names = xml::edit::Names::new(cfg);
                let refs = xml::edit::refs(
                    cfg,
                    &names,
                    if id == cfg.root.header.id {
                        &cfg.root_id
                    } else {
                        id
                    },
                )
                .map_err(format_error)?;
                let mut ids = std::collections::HashSet::new();
                let mut nodes = Vec::new();
                for (entry, _) in refs {
                    let owner = if entry == cfg.root_id {
                        cfg.root.header.id.clone()
                    } else {
                        entry.split('.').next().unwrap_or(&entry).to_string()
                    };
                    if let Ok(i) = item(cfg, &owner) {
                        if ids.insert(i.header.id.clone()) {
                            nodes.push(crate::tree::object_node(cfg, i));
                        }
                    } else if ids.insert(entry.clone()) {
                        nodes.push(json!({"id":format!("record:{entry}"),"label":xml::edit::whose(cfg,&entry),"kind":"record","hasChildren":false}));
                    }
                }
                Ok(json!(nodes))
            }
            "model.properties" => {
                let cfg = self.cfg()?;
                let id = text(p, "id")?;
                let names = xml::edit::Names::new(cfg);
                let o = xml::edit::find(
                    cfg,
                    if id == cfg.root.header.id {
                        &cfg.root_id
                    } else {
                        id
                    },
                )
                .ok_or_else(|| bad("Неизвестный объект"))?;
                let source = xml::edit::export(cfg, &names, &o).map_err(format_error)?;
                let props =
                    xml::edit::props_of(&source, o.kind(), o.sub.map(|s| s.header.id.as_str()))
                        .map_err(format_error)?;
                let captions = xml::captions::Captions::load();
                Ok(
                    json!({"id":id,"properties":props.iter().map(|p|property(p,&captions,xml::edit::available(&props,&p.tag))).collect::<Vec<_>>()}),
                )
            }
            "model.setProperty" => {
                self.revision(p)?;
                let id = text(p, "id")?;
                let name = text(p, "name")?;
                let value = p.get("value").ok_or_else(|| bad("Отсутствует value"))?;
                let cfg = self.cfg()?;
                let mut staged = Config::open(&cfg.save()).map_err(format_error)?;
                edit(&mut staged, id, name, value)?;
                let checked = Config::open(&staged.save()).map_err(format_error)?;
                // Import can normalize or omit a value. Never acknowledge a
                // write unless the public library roundtrip retains that value.
                let names = xml::edit::Names::new(&checked);
                let object = xml::edit::find(
                    &checked,
                    if id == checked.root.header.id {
                        &checked.root_id
                    } else {
                        id
                    },
                )
                .ok_or_else(|| bad("После правки объект недоступен"))?;
                let source = xml::edit::export(&checked, &names, &object).map_err(format_error)?;
                let properties = xml::edit::props_of(
                    &source,
                    object.kind(),
                    object.sub.map(|s| s.header.id.as_str()),
                )
                .map_err(format_error)?;
                let retained = properties
                    .iter()
                    .find(|p| p.tag == name)
                    .ok_or_else(|| bad("Свойство не сохранено библиотекой"))?;
                if property(retained, &xml::captions::Captions::load(), true)["value"] != *value {
                    return Err(bad(
                        "Библиотека не сохранила заданное значение; модель не изменена",
                    ));
                }
                self.config = Some(checked);
                self.dirty = true;
                self.revision += 1;
                Ok(self.state())
            }
            "model.save" => {
                self.revision(p)?;
                let cfg = self.cfg()?;
                if p.get("path").is_some_and(|v| !v.is_string()) {
                    return Err(bad("path: требуется строка"));
                }
                let format = p
                    .get("format")
                    .map(|v| v.as_str().ok_or_else(|| bad("format: требуется строка")))
                    .transpose()?
                    .unwrap_or("binary");
                if !["binary", "xml"].contains(&format) {
                    return Err(bad("format: binary или xml"));
                }
                if self.xml_source && p.get("path").is_none() {
                    return Err(bad("Для XML выберите бинарный файл или новый XML-каталог"));
                }
                let destination = p
                    .get("path")
                    .and_then(Value::as_str)
                    .or(self.source.as_deref())
                    .ok_or_else(|| bad("Выберите путь сохранения"))?;
                let path = Path::new(destination);
                if format == "xml" {
                    export(cfg, path).map_err(format_error)?;
                } else {
                    atomic_write(path, &cfg.save()).map_err(format_error)?;
                }
                self.source = Some(destination.to_string());
                self.xml_source = format == "xml";
                self.dirty = false;
                self.revision += 1;
                Ok(self.state())
            }
            _ => Err((-32601, format!("Unknown method: {method}"))),
        }
    }
}
fn property(p: &xml::edit::Prop, c: &xml::captions::Captions, available: bool) -> Value {
    use xml::edit::{Kind, Value as V};
    let (kind, value, editable, options) = match (&p.kind, &p.value) {
        (Kind::Str, V::Text(s)) => ("string", json!(s), true, None),
        (Kind::Ml, V::Ml(v)) => (
            "string",
            json!(v
                .iter()
                .find(|(l, _)| l == "ru")
                .map(|(_, s)| s.as_str())
                .unwrap_or("")),
            true,
            None,
        ),
        (Kind::Bool, V::Text(s)) => ("boolean", json!(s == "true"), true, None),
        (Kind::Num, V::Text(s)) => match s.parse::<serde_json::Number>() {
            Ok(n) => ("number", json!(n), true, None),
            Err(_) => ("readonly", json!(s), false, None),
        },
        (Kind::Enum(values), V::Text(s)) => (
            "enum",
            json!(s),
            true,
            Some(values.iter().map(|(_, v)| *v).collect::<Vec<_>>()),
        ),
        _ => ("readonly", json!(p.shown()), false, None),
    };
    let mut dto = json!({"name":p.tag,"label":c.prop(&p.tag).0,"kind":kind,"value":value,"editable":editable && available});
    if let Some(options) = options {
        dto["options"] = json!(options);
    }
    dto
}
fn edit(cfg: &mut Config, id: &str, name: &str, value: &Value) -> Result<()> {
    use xml::edit::{self, Kind, Value as V};
    let mut names = edit::Names::new(cfg);
    let o = edit::find(
        cfg,
        if id == cfg.root.header.id {
            &cfg.root_id
        } else {
            id
        },
    )
    .ok_or_else(|| bad("Неизвестный объект"))?;
    let source = edit::export(cfg, &names, &o).map_err(format_error)?;
    let props = edit::props_of(&source, o.kind(), o.sub.map(|s| s.header.id.as_str()))
        .map_err(format_error)?;
    let prop = props
        .iter()
        .find(|p| p.tag == name)
        .ok_or_else(|| bad("Неизвестное свойство"))?;
    if !edit::available(&props, name) {
        return Err(bad(
            "Свойство недоступно при текущих значениях остальных свойств",
        ));
    }
    let converted = match (&prop.kind, &prop.value) {
        (Kind::Str, _) => V::Text(
            value
                .as_str()
                .ok_or_else(|| bad("Требуется строка"))?
                .into(),
        ),
        (Kind::Ml, V::Ml(languages)) => {
            let text = value.as_str().ok_or_else(|| bad("Требуется строка"))?;
            let mut v = languages.clone();
            if let Some((_, s)) = v.iter_mut().find(|(l, _)| l == "ru") {
                *s = text.into();
            } else {
                v.push(("ru".into(), text.into()));
            }
            V::Ml(v)
        }
        (Kind::Bool, _) => V::Text(
            value
                .as_bool()
                .ok_or_else(|| bad("Требуется boolean"))?
                .to_string(),
        ),
        (Kind::Num, _) => V::Text(
            value
                .as_number()
                .ok_or_else(|| bad("Требуется число"))?
                .to_string(),
        ),
        (Kind::Enum(options), _) => {
            let s = value.as_str().ok_or_else(|| bad("Требуется enum"))?;
            if !options.iter().any(|(_, v)| *v == s) {
                return Err(bad("Недопустимое значение enum"));
            }
            V::Text(s.into())
        }
        _ => return Err(bad("Свойство доступно только для чтения")),
    };
    if name == "Name" {
        let new = value.as_str().ok_or_else(|| bad("Требуется имя"))?;
        if !new
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
            || !new.chars().all(|c| c.is_alphanumeric() || c == '_')
        {
            return Err(bad("Неверное имя: используйте буквы, цифры и _"));
        }
        let i = item(cfg, id)?;
        let siblings = if let Some(parent) = parent_of(cfg, id) {
            if parent.starts_with("group:") {
                cfg.groups
                    .iter()
                    .find(|g| format!("group:{}", g.class) == parent)
                    .map(|g| g.items.iter().collect::<Vec<_>>())
                    .unwrap_or_default()
            } else {
                item(cfg, &parent)?.children.iter().collect()
            }
        } else {
            vec![]
        };
        if siblings.iter().any(|s| {
            s.header.id != id
                && s.kind == i.kind
                && s.header.name.to_lowercase() == new.to_lowercase()
        }) {
            return Err(bad("Имя уже существует"));
        }
        if o.sub.is_none() && o.owner.is_none() && o.full.is_none() && o.class != "Configuration" {
            edit::rename(cfg, &mut names, id, new).map_err(bad)?;
            return Ok(());
        }
    }
    let updated = edit::set(&source, prop, &converted);
    let record = edit::import(&updated, &names, &o).map_err(bad)?;
    let entry = o.entry.clone();
    edit::store(cfg, &entry, &record).map_err(format_error)?;
    Ok(())
}
fn walk<'a>(i: &'a Item, parent: Option<String>, nodes: &mut Vec<(&'a Item, Option<String>)>) {
    nodes.push((i, parent));
    for child in &i.children {
        walk(child, Some(i.header.id.clone()), nodes);
    }
}
fn all_nodes(cfg: &Config) -> Vec<(&Item, Option<String>)> {
    let mut nodes = Vec::new();
    walk(&cfg.root, None, &mut nodes);
    for g in &cfg.groups {
        for i in &g.items {
            walk(i, Some(format!("group:{}", g.class)), &mut nodes);
        }
    }
    nodes
}
fn item<'a>(cfg: &'a Config, id: &str) -> Result<&'a Item> {
    all_nodes(cfg)
        .into_iter()
        .find(|(i, _)| i.header.id == id)
        .map(|(i, _)| i)
        .ok_or_else(|| bad("Неизвестный объект"))
}
fn parent_of(cfg: &Config, id: &str) -> Option<String> {
    all_nodes(cfg)
        .into_iter()
        .find(|(i, _)| i.header.id == id)
        .and_then(|(_, p)| p)
}
fn is_xml(path: &Path) -> bool {
    path.is_dir()
        || path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("xml"))
}
fn open(path: &Path) -> std::result::Result<Config, String> {
    if is_xml(path) {
        let source = if path.is_dir() {
            path
        } else {
            let contents = fs::read_to_string(path).map_err(|e| e.to_string())?;
            let document = xml::imp::dom::parse(&contents)?;
            if document.name != "MetaDataObject" {
                return Err("XML: выберите корневой файл метаданных".into());
            }
            if document.child("ExternalDataProcessor").is_some() {
                path
            } else if document.child("Configuration").is_some()
                && path
                    .file_name()
                    .is_some_and(|name| name == "Configuration.xml")
            {
                path.parent().ok_or("XML has no parent")?
            } else {
                return Err("XML: выберите Configuration.xml или корень внешней обработки".into());
            }
        };
        let imported = xml::imp::import(source, None)?;
        if !imported.refused.is_empty() {
            return Err(format!("XML import refused: {:?}", imported.refused));
        }
        Config::open(&xml::imp::cf(source, &imported)?)
    } else {
        Config::open(&fs::read(path).map_err(|e| e.to_string())?)
    }
}
fn temporary(path: &Path) -> PathBuf {
    path.parent().unwrap_or(Path::new(".")).join(format!(
        ".cf-extension-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
fn atomic_write(path: &Path, bytes: &[u8]) -> std::result::Result<(), String> {
    let tmp = temporary(path);
    let mut f = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        drop(f);
        Config::open(bytes)?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
fn export(cfg: &Config, path: &Path) -> std::result::Result<(), String> {
    if path.exists()
        && (!path.is_dir()
            || fs::read_dir(path)
                .map_err(|e| e.to_string())?
                .next()
                .is_some())
    {
        return Err(
            "XML: выберите новый или пустой каталог; существующие файлы не изменены".into(),
        );
    }
    let tmp = temporary(path);
    fs::create_dir(&tmp).map_err(|e| e.to_string())?;
    let result = (|| {
        let report = xml::export(cfg, &tmp)?;
        if !report.errors.is_empty() || !report.skipped.is_empty() {
            return Err(format!(
                "XML export incomplete: {:?}; {:?}",
                report.errors, report.skipped
            ));
        }
        let root = if cfg.root.kind == "Configuration" {
            tmp.clone()
        } else {
            tmp.join(format!("{}.xml", cfg.root.header.name))
        };
        open(&root)?;
        if path.exists() {
            fs::remove_dir(path).map_err(|e| e.to_string())?;
        }
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&tmp);
    }
    result
}

/// Only entries enumerated by the format library and readable in the current model.
pub fn modules(cfg: &Config) -> Vec<Value> {
    cfg.module_entries()
        .into_iter()
        .filter_map(|(id, label)| {
            cfg.module_text(&id)?;
            let (owner, role) = all_nodes(cfg).into_iter().find_map(|(i, _)| {
                i.files
                    .iter()
                    .find(|(entry, role)| entry == &id && cf_unwrapper::meta::is_module_role(role))
                    .map(|(_, role)| (i.header.id.clone(), *role))
            })?;
            Some(json!({"id":id,"ownerId":owner,"role":role,"label":label}))
        })
        .collect()
}
