//! Presentation hierarchy only. Metadata and format parsing remain in cf_unwrapper.
use cf_unwrapper::meta::{self, Config, Item};
use serde_json::{json, Value};

const COMMON: &[&str] = &[
    "Subsystem",
    "CommonModule",
    "SessionParameter",
    "Role",
    "CommonAttribute",
    "ExchangePlan",
    "FilterCriterion",
    "EventSubscription",
    "ScheduledJob",
    "Bot",
    "FunctionalOption",
    "FunctionalOptionsParameter",
    "DefinedType",
    "SettingsStorage",
    "CommonCommand",
    "CommandGroup",
    "CommonForm",
    "CommonTemplate",
    "CommonPicture",
    "XDTOPackage",
    "WebService",
    "HTTPService",
    "WSReference",
    "WebSocketClient",
    "IntegrationService",
    "PaletteColor",
    "StyleItem",
    "Style",
    "Language",
];
const TOP: &[&str] = &[
    "Constant",
    "Catalog",
    "Document",
    "DocumentJournal",
    "Enum",
    "Report",
    "DataProcessor",
    "ChartOfCharacteristicTypes",
    "ChartOfAccounts",
    "ChartOfCalculationTypes",
    "InformationRegister",
    "AccumulationRegister",
    "AccountingRegister",
    "CalculationRegister",
    "BusinessProcess",
    "Task",
    "ExternalDataSource",
];
const DOCUMENT_GROUPS: &[&str] = &["DocumentNumerator", "Sequence"];

fn shown(kind: &str) -> &'static [&'static str] {
    match kind {
        "Catalog"
        | "Document"
        | "Report"
        | "DataProcessor"
        | "ExchangePlan"
        | "ChartOfCharacteristicTypes"
        | "ChartOfCalculationTypes"
        | "BusinessProcess" => &["Attribute", "TabularSection", "Form", "Command", "Template"],
        "ExternalDataProcessor" | "ExternalReport" => {
            &["Attribute", "TabularSection", "Form", "Template"]
        }
        "TabularSection" => &["Attribute"],
        "InformationRegister"
        | "AccumulationRegister"
        | "AccountingRegister"
        | "CalculationRegister" => &[
            "Dimension",
            "Resource",
            "Attribute",
            "Form",
            "Command",
            "Template",
        ],
        "ChartOfAccounts" => &[
            "Attribute",
            "AccountingFlag",
            "ExtDimensionAccountingFlag",
            "TabularSection",
            "Form",
            "Command",
            "Template",
        ],
        "Task" => &[
            "AddressingAttribute",
            "Attribute",
            "TabularSection",
            "Form",
            "Command",
            "Template",
        ],
        "Enum" => &["EnumValue", "Form", "Command", "Template"],
        "DocumentJournal" => &["Column", "Form", "Command", "Template"],
        "FilterCriterion" => &["Form", "Command"],
        "SettingsStorage" => &["Form", "Template"],
        "HTTPService" => &["URLTemplate"],
        "URLTemplate" => &["Method"],
        "WebService" => &["Operation"],
        "Operation" => &["Parameter"],
        "IntegrationService" => &["IntegrationServiceChannel"],
        _ => &[],
    }
}
fn title(class: &str) -> &str {
    if class == "DocumentNumerator" {
        return "Нумераторы";
    }
    meta::CLASSES
        .iter()
        .find(|c| c.1 == class)
        .map(|c| c.2)
        .or_else(|| meta::SUBKINDS.iter().find(|c| c.0 == class).map(|c| c.1))
        .unwrap_or(match class {
            "WebSocketClient" => "WebSocket-клиенты",
            "PaletteColor" => "Цвета палитры",
            "Style" => "Стили",
            "CalculationRegister" => "Регистры расчета",
            "ExternalDataSource" => "Внешние источники данных",
            "DocumentNumerator" => "Нумераторы",
            _ => class,
        })
}
fn group(id: &str, label: &str, class: &str, parent: &str, has_children: bool) -> Value {
    json!({"id":id,"label":label,"kind":"group","metadataKind":class,"typeLabel":label,"parentId":parent,"hasChildren":has_children})
}
fn child_kinds(item: &Item) -> Vec<&str> {
    let mut kinds = shown(item.kind).to_vec();
    for (kind, _) in meta::SUBKINDS {
        if !kinds.contains(kind) && item.children.iter().any(|c| c.kind == *kind) {
            kinds.push(kind);
        }
    }
    for child in &item.children {
        if !kinds.contains(&child.kind) {
            kinds.push(child.kind);
        }
    }
    kinds
}
fn find_in<'a>(
    item: &'a Item,
    id: &str,
    parent: Option<&'a Item>,
) -> Option<(&'a Item, Option<&'a Item>)> {
    if item.header.id == id {
        return Some((item, parent));
    }
    item.children
        .iter()
        .find_map(|c| find_in(c, id, Some(item)))
}
fn find<'a>(cfg: &'a Config, id: &str) -> Option<(&'a Item, Option<&'a Item>)> {
    find_in(&cfg.root, id, None).or_else(|| {
        cfg.groups
            .iter()
            .flat_map(|g| &g.items)
            .find_map(|i| find_in(i, id, None))
    })
}
fn class_parent(cfg: &Config, class: &str) -> String {
    if COMMON.contains(&class) {
        "common".into()
    } else if DOCUMENT_GROUPS.contains(&class) {
        "group:Document".into()
    } else {
        cfg.root.header.id.clone()
    }
}
fn class_node(cfg: &Config, class: &str) -> Value {
    let count = cfg
        .groups
        .iter()
        .find(|g| g.class == class)
        .map_or(0, |g| g.items.len());
    group(
        &format!("group:{class}"),
        title(class),
        class,
        &class_parent(cfg, class),
        count > 0 || class == "Document",
    )
}
fn with_parent(item: &Item, parent: Option<&str>) -> Value {
    let label = if item.header.name.is_empty() {
        &item.header.id
    } else {
        &item.header.name
    };
    let mut node = json!({"id":item.header.id,"label":label,"kind":item.kind,"metadataKind":item.kind,
        "typeLabel":meta::kind_ru(item.kind),"hasChildren":!child_kinds(item).is_empty() || item.kind == "Configuration"});
    if let Some(parent) = parent {
        node["parentId"] = json!(parent);
    }
    node
}
pub fn from_structural(item: &Item, parent: Option<&str>) -> Value {
    let parent = parent.map(|p| {
        if p.starts_with("group:") {
            p.to_owned()
        } else {
            format!("children:{p}:{}", item.kind)
        }
    });
    with_parent(item, parent.as_deref())
}
pub fn object_node(cfg: &Config, item: &Item) -> Value {
    let parent = if item.header.id == cfg.root.header.id {
        None
    } else if let Some((_, Some(owner))) = find(cfg, &item.header.id) {
        Some(format!("children:{}:{}", owner.header.id, item.kind))
    } else {
        Some(format!("group:{}", item.kind))
    };
    let mut node = with_parent(item, parent.as_deref());
    if crate::model::modules(cfg)
        .iter()
        .any(|m| m["ownerId"] == item.header.id)
    {
        node["hasChildren"] = json!(true);
    }
    node
}
pub fn children(cfg: &Config, parent: Option<&str>) -> Result<Vec<Value>, String> {
    let Some(parent) = parent else {
        return Ok(vec![object_node(cfg, &cfg.root)]);
    };
    if parent == cfg.root.header.id && cfg.root.kind == "Configuration" {
        let mut nodes = vec![group("common", "Общие", "Common", parent, true)];
        nodes.extend(TOP.iter().map(|class| class_node(cfg, class)));
        for g in &cfg.groups {
            if !COMMON.contains(&g.class)
                && !TOP.contains(&g.class)
                && !DOCUMENT_GROUPS.contains(&g.class)
            {
                nodes.push(class_node(cfg, g.class));
            }
        }
        nodes.extend(module_nodes(cfg, parent));
        return Ok(nodes);
    }
    if parent == "common" && cfg.root.kind == "Configuration" {
        return Ok(COMMON.iter().map(|class| class_node(cfg, class)).collect());
    }
    if let Some(class) = parent.strip_prefix("group:") {
        if cfg.root.kind != "Configuration"
            || !(COMMON.contains(&class)
                || TOP.contains(&class)
                || DOCUMENT_GROUPS.contains(&class)
                || cfg.groups.iter().any(|g| g.class == class))
        {
            return Err("Неизвестная группа".into());
        }
        let mut nodes = if class == "Document" {
            DOCUMENT_GROUPS.iter().map(|c| class_node(cfg, c)).collect()
        } else {
            vec![]
        };
        if let Some(g) = cfg.groups.iter().find(|g| g.class == class) {
            nodes.extend(g.items.iter().map(|i| object_node(cfg, i)));
        }
        return Ok(nodes);
    }
    if let Some(rest) = parent.strip_prefix("children:") {
        let (owner, kind) = rest.split_once(':').ok_or("Некорректная группа")?;
        let (item, _) = find(cfg, owner).ok_or("Неизвестный объект")?;
        if !child_kinds(item).contains(&kind) {
            return Err("Неизвестная вложенная группа".into());
        }
        return Ok(item
            .children
            .iter()
            .filter(|c| c.kind == kind)
            .map(|c| object_node(cfg, c))
            .collect());
    }
    let (item, _) = find(cfg, parent).ok_or("Неизвестный объект")?;
    let mut nodes: Vec<Value> = child_kinds(item)
        .iter()
        .map(|kind| {
            group(
                &format!("children:{parent}:{kind}"),
                title(kind),
                kind,
                parent,
                item.children.iter().any(|c| c.kind == *kind),
            )
        })
        .collect();
    nodes.extend(module_nodes(cfg, parent));
    Ok(nodes)
}
fn module_nodes(cfg: &Config, owner: &str) -> Vec<Value> {
    crate::model::modules(cfg).into_iter().filter(|m|m["ownerId"]==owner).map(|m|json!({"id":format!("module:{}",m["id"].as_str().unwrap()),"label":m["label"],"kind":"module","role":m["role"],"metadataKind":m["role"],"parentId":owner,"hasChildren":false})).collect()
}
pub fn node(cfg: &Config, id: &str) -> Result<Value, String> {
    if let Some(entry) = id.strip_prefix("module:") {
        return crate::model::modules(cfg)
            .into_iter()
            .find(|m| m["id"] == entry)
            .and_then(|m| {
                module_nodes(cfg, m["ownerId"].as_str()?)
                    .into_iter()
                    .find(|n| n["id"] == id)
            })
            .ok_or("Неизвестный модуль".into());
    }
    if let Some((item, _)) = find(cfg, id) {
        return Ok(object_node(cfg, item));
    }
    if id == "common" && cfg.root.kind == "Configuration" {
        return Ok(group(
            "common",
            "Общие",
            "Common",
            &cfg.root.header.id,
            true,
        ));
    }
    if let Some(class) = id.strip_prefix("group:") {
        if cfg.root.kind == "Configuration"
            && (COMMON.contains(&class)
                || TOP.contains(&class)
                || DOCUMENT_GROUPS.contains(&class)
                || cfg.groups.iter().any(|g| g.class == class))
        {
            return Ok(class_node(cfg, class));
        }
    }
    if let Some(rest) = id.strip_prefix("children:") {
        if let Some((owner, _)) = rest.split_once(':') {
            return children(cfg, Some(owner))?
                .into_iter()
                .find(|n| n["id"] == id)
                .ok_or("Неизвестная группа".into());
        }
    }
    Err("Неизвестный узел".into())
}
