//! Диагностика BSL поверх единой модели: сессия анализа (LSP full-sync) и перевод позиций
//! (1-based, знаки) в UTF-16 (0-based), как требуют редакторы. Сами проверки — в `mod.rs` (`check_all`):
//! синтаксис, вызовы методов модуля и обращения к известным коллекциям метаданных.
//!
//! Проверка «переменная не определена» без полного контекста платформы намеренно отсутствует: без HBK-справки
//! она ложно срабатывала бы на глобальные имена платформы. Неизвестные и динамические имена не диагностируются.

use super::{check_all, Error, Site};
use cf_unwrapper::meta::Config;
use serde_json::{json, Value};

/// Тексты крупнее этого не анализируются (пустая диагностика): защита от зависаний на гигантских буферах.
const ANALYSIS_LIMIT: usize = 1_000_000;

/// Место компиляции по роли модуля из библиотечного списка.
pub fn site(role: &str) -> Site {
    match role {
        "Form" => Site::Form,
        r if r.ends_with("CommandModule") => Site::Command,
        "Module"
        | "ObjectModule"
        | "ManagerModule"
        | "RecordSetModule"
        | "ValueManagerModule"
        | "SessionModule"
        | "ExternalConnectionModule"
        | "OrdinaryApplicationModule"
        | "ManagedApplicationModule" => Site::Server,
        _ => Site::Other,
    }
}

/// Все диагностики текста модуля по метаданным открытой конфигурации.
pub fn diagnostics(text: &str, role: &str, cfg: &Config) -> Vec<Error> {
    if text.chars().count() > ANALYSIS_LIMIT {
        return Vec::new();
    }
    let mut exists = |class: &str, name: &str| {
        cfg.groups.iter().any(|g| {
            g.class == class
                && g.items
                    .iter()
                    .any(|i| i.header.name.eq_ignore_ascii_case(name))
        })
    };
    check_all(text, site(role), Some(&mut exists))
}

/// Позиция (строка и колонка от 1, счёт знаками) → (строка от 0, символ в UTF-16 единицах).
fn position(text: &str, line1: u32, col1: u32) -> (u32, u32) {
    let line_text = text
        .split('\n')
        .nth((line1.max(1) - 1) as usize)
        .unwrap_or("");
    let character = line_text
        .chars()
        .take((col1.max(1) - 1) as usize)
        .map(|c| c.len_utf16() as u32)
        .sum();
    (line1.max(1) - 1, character)
}

/// Диапазон ошибки: от позиции до конца слова в этой позиции (если оно там есть), иначе нулевой.
fn range(text: &str, e: &Error) -> Value {
    let (line, character) = position(text, e.line, e.col);
    let line_text = text.split('\n').nth(line as usize).unwrap_or("");
    let mut end = character;
    for c in line_text.chars().skip((e.col.max(1) - 1) as usize) {
        if c.is_alphanumeric() || c == '_' {
            end += c.len_utf16() as u32;
        } else {
            break;
        }
    }
    json!({"start":{"line":line,"character":character},"end":{"line":line,"character":end}})
}

/// Открытый документ анализа: идентичность привязана к модели, текст — к последнему didChange.
#[derive(Clone)]
struct Doc {
    module: String,
    version: i64,
    text: String,
}

/// Сессия анализа LSP-документов. Живёт внутри модели: успешный open/close модели очищает её.
/// Сама в Config не пишет — анализ по тексту, отдельно от применённого текста.
#[derive(Default)]
pub struct Session {
    docs: std::collections::HashMap<String, Doc>,
}

impl Session {
    /// `textDocument/didOpen`: версия запоминается, диагностика публикуется сразу.
    pub fn open(
        &mut self,
        uri: &str,
        module: &str,
        version: i64,
        text: &str,
        cfg: &Config,
        role: &str,
    ) -> Value {
        self.docs.insert(
            uri.to_string(),
            Doc {
                module: module.into(),
                version,
                text: text.into(),
            },
        );
        self.publish(uri, version, text, cfg, role)
    }

    /// `textDocument/didChange` (full sync): версия строго вперёд.
    pub fn change(
        &mut self,
        uri: &str,
        version: i64,
        text: &str,
        cfg: &Config,
        role: &str,
    ) -> Result<Value, (i32, String)> {
        let Some(doc) = self.docs.get_mut(uri) else {
            return Err((-32602, "Неизвестный документ анализа".into()));
        };
        if version <= doc.version {
            return Err((-32602, "Версия документа изменяется только вперёд".into()));
        }
        doc.version = version;
        doc.text = text.into();
        Ok(self.publish(uri, version, text, cfg, role))
    }

    /// `textDocument/didClose`: документ забывается, диагностика очищается.
    pub fn close(&mut self, uri: &str) -> Option<Value> {
        self.docs.remove(uri).map(|d| params(uri, d.version, &[]))
    }

    /// Смена/закрытие модели: всё забыто, для каждого документа — пустая диагностика.
    pub fn clear(&mut self) -> Vec<Value> {
        self.docs
            .drain()
            .map(|(uri, d)| params(&uri, d.version, &[]))
            .collect()
    }

    pub fn module_of(&self, uri: &str) -> Option<&str> {
        self.docs.get(uri).map(|d| d.module.as_str())
    }

    fn publish(&self, uri: &str, version: i64, text: &str, cfg: &Config, role: &str) -> Value {
        let errors = diagnostics(text, role, cfg);
        let list: Vec<Value> = errors
            .iter()
            .map(|e| json!({"range":range(text,e),"severity":1,"source":"Конструктор BSL","message":e.msg}))
            .collect();
        params(uri, version, &list)
    }
}

fn params(uri: &str, version: i64, diagnostics: &[Value]) -> Value {
    json!({"uri":uri,"version":version,"diagnostics":diagnostics})
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Имена «есть в конфигурации»: Products — есть, НетТакого — нет.
    fn fixture_exists(class: &str, name: &str) -> bool {
        class == "Catalog" && name.eq_ignore_ascii_case("Products")
    }

    fn member(src: &str) -> Vec<(u32, u32, String)> {
        let mut exists = fixture_exists;
        super::super::member_errors(&super::super::parse(src).unwrap_or_default(), &mut exists)
            .into_iter()
            .map(|e| (e.line, e.col, e.msg))
            .collect()
    }

    #[test]
    fn known_collection_member_is_checked() {
        let p = |b: &str| format!("Процедура А()\n{b}\nКонецПроцедуры\n");
        assert_eq!(
            member(&p("\tСправочники.НетТакого.ПустаяСсылка();")),
            [(
                2,
                14,
                "Объект метаданных не найден (Справочники.НетТакого)".into()
            )]
        );
        assert_eq!(member(&p("\tСправочники.Products.ПустаяСсылка();")), []);
        assert_eq!(
            member(&p("\tCatalogs.Missing.One();")),
            [(
                2,
                11,
                "Объект метаданных не найден (Catalogs.Missing)".into()
            )]
        );
        // неизвестные и динамические коллекции не проверяются
        assert_eq!(member(&p("\tЗапрос.Выполнить();")), []);
        assert_eq!(member(&p("\tКоллекция.НетТакого.Метод();")), []);
        // цепочка: внешнее имя не проверяется, первый уровень — да
        assert_eq!(
            member(&p("\tПоле = Справочники.Products.Реквизиты.Нет;")),
            []
        );
    }

    #[test]
    fn utf16_positions() {
        // Кириллица (по 1 UTF-16) и astral-символ (по 2): колонка считается знаками, переводится в UTF-16.
        let text = "Процедура А() // 😀\r\n\tБ = Х;\r\nКонецПроцедуры\r\n";
        let (line, ch) = position(text, 2, 6);
        assert_eq!(
            (line, ch),
            (1, 5),
            "перед 'Х' — пять знаков: таб, Б, пробел, =, пробел"
        );
        let (line, ch) = position(text, 1, 19);
        assert_eq!(
            (line, ch),
            (0, 19),
            "emoji до позиции даёт 2 UTF-16 на знак"
        );
        let r = range(
            text,
            &Error {
                line: 2,
                col: 6,
                msg: String::new(),
            },
        );
        assert_eq!(r["start"]["character"], 5);
        assert_eq!(r["end"]["character"], 6, "имя из одной буквы");
    }

    #[test]
    fn site_by_role() {
        assert_eq!(site("Form"), Site::Form);
        assert_eq!(site("ObjectModule"), Site::Server);
        assert_eq!(site("SessionModule"), Site::Server);
        assert_eq!(site("SomethingElse"), Site::Other);
    }
}
