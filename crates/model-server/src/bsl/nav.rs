//! Подсказки (Ctrl+Пробел) и переходы к определению (F12) по открытой конфигурации.
//!
//! Происхождение: ядро `complete` из constructor `src/bsl/complete.rs` (собственный код проекта) —
//! таблицы коллекций менеджеров, разбор заголовков методов, сбор переменных, определение слова
//! под курсором. Правки переноса: убрана зависимость от справки платформы (`ctx`/HBK) — вместо
//! членов типов из справки подсказываются только доказуемые по `Config` имена (объекты коллекций,
//! реквизиты и табличные части, экспортные методы общих модулей и менеджеров) плюс небольшой
//! собственный набор широко известных методов менеджеров/объектов (создать, найти, выбрать,
//! записать — RU/EN). Неизвестные цели не выдумываются: пустой результат.

use super::lex::{self, Kw, Token, T};

/// Метаданные конфигурации для подсказки — адаптер над `cf_unwrapper::meta::Config`.
pub trait Meta {
    /// Имена объектов класса (`Catalog`, `CommonModule`, …).
    fn names(&self, class: &str) -> Vec<String>;
    /// Текст модуля объекта (`role` — `Module`, `ManagerModule`, …).
    fn module(&self, class: &str, name: &str, role: &str) -> Option<String>;
    /// Подчиненные объекты (вид, имя): реквизиты и табличные части объекта, реквизиты табличной части.
    fn fields(&self, class: &str, name: &str, ts: Option<&str>) -> Vec<(&'static str, String)>;
    /// Идентификатор объекта в дереве метаданных (UUID записи).
    fn id(&self, class: &str, name: &str) -> Option<String>;
    /// Заголовок класса для подсказки (`Catalog` → «Справочники»).
    fn title(&self, class: &str) -> String {
        class.to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemKind {
    Keyword,
    Var,
    Method,
    Property,
    Module,
    Object,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    /// Что вставить.
    pub insert: String,
    pub kind: ItemKind,
    /// Вид объекта или строка заголовка.
    pub detail: String,
}

/// Коллекции менеджеров глобального контекста: английское имя свойства и класс метаданных.
/// Происхождение: таблица MANAGERS из constructor `src/bsl/complete.rs`.
const MANAGERS: &[(&str, &str)] = &[
    ("Catalogs", "Catalog"),
    ("Documents", "Document"),
    ("Enums", "Enum"),
    ("Constants", "Constant"),
    ("InformationRegisters", "InformationRegister"),
    ("AccumulationRegisters", "AccumulationRegister"),
    ("AccountingRegisters", "AccountingRegister"),
    ("CalculationRegisters", "CalculationRegister"),
    ("DataProcessors", "DataProcessor"),
    ("Reports", "Report"),
    ("ChartsOfCharacteristicTypes", "ChartOfCharacteristicTypes"),
    ("ChartsOfAccounts", "ChartOfAccounts"),
    ("ChartsOfCalculationTypes", "ChartOfCalculationTypes"),
    ("BusinessProcesses", "BusinessProcess"),
    ("Tasks", "Task"),
    ("ExchangePlans", "ExchangePlan"),
    ("DocumentJournals", "DocumentJournal"),
    ("Sequences", "Sequence"),
];

/// Русские имена коллекций менеджеров. Происхождение: RU_MANAGERS из constructor `src/bsl/complete.rs`.
pub const RU_MANAGERS: &[(&str, &str)] = &[
    ("Справочники", "Catalogs"),
    ("Документы", "Documents"),
    ("Перечисления", "Enums"),
    ("Константы", "Constants"),
    ("РегистрыСведений", "InformationRegisters"),
    ("РегистрыНакопления", "AccumulationRegisters"),
    ("РегистрыБухгалтерии", "AccountingRegisters"),
    ("РегистрыРасчета", "CalculationRegisters"),
    ("Обработки", "DataProcessors"),
    ("Отчеты", "Reports"),
    ("ПланыВидовХарактеристик", "ChartsOfCharacteristicTypes"),
    ("ПланыСчетов", "ChartsOfAccounts"),
    ("ПланыВидовРасчета", "ChartsOfCalculationTypes"),
    ("БизнесПроцессы", "BusinessProcesses"),
    ("Задачи", "Tasks"),
    ("ПланыОбмена", "ExchangePlans"),
    ("ЖурналыДокументов", "DocumentJournals"),
    ("Последовательности", "Sequences"),
];

/// Класс метаданных по имени коллекции (RU/EN, без учёта регистра).
pub fn collection_class(name: &str) -> Option<&'static str> {
    MANAGERS
        .iter()
        .find(|(en, _)| en.eq_ignore_ascii_case(name))
        .map(|(_, class)| *class)
        .or_else(|| {
            RU_MANAGERS
                .iter()
                .find(|(ru, _)| ru.eq_ignore_ascii_case(name))
                .and_then(|(_, en)| MANAGERS.iter().find(|(e, _)| e == en))
                .map(|(_, class)| *class)
        })
}

const KEYWORDS: &[&str] = &[
    "Если",
    "Тогда",
    "ИначеЕсли",
    "Иначе",
    "КонецЕсли",
    "Для",
    "Каждого",
    "Из",
    "По",
    "Цикл",
    "КонецЦикла",
    "Пока",
    "Возврат",
    "Продолжить",
    "Прервать",
    "Перем",
    "Экспорт",
    "Знач",
    "Попытка",
    "Исключение",
    "КонецПопытки",
    "ВызватьИсключение",
    "Новый",
    "И",
    "Или",
    "Не",
    "Истина",
    "Ложь",
    "Неопределено",
    "NULL",
    "Выполнить",
    "Асинх",
    "Ждать",
    "Процедура",
    "КонецПроцедуры",
    "Функция",
    "КонецФункции",
    "ДобавитьОбработчик",
    "УдалитьОбработчик",
];

/// Широко известные методы менеджера объектов: RU, EN, чему равен результат.
/// Собственный минимальный набор вместо справки платформы; семантика динамических типов не заявляется.
const MANAGER_METHODS: &[(&str, &str, Ret)] = &[
    ("СоздатьЭлемент", "CreateItem", Ret::Object),
    ("СоздатьДокумент", "CreateDocument", Ret::Object),
    ("СоздатьБизнесПроцесс", "CreateBusinessProcess", Ret::Object),
    ("СоздатьЗадачу", "CreateTask", Ret::Object),
    ("СоздатьМенеджерЗаписи", "CreateRecordManager", Ret::Object),
    ("СоздатьНаборЗаписей", "CreateRecordSet", Ret::Object),
    ("ПустаяСсылка", "EmptyRef", Ret::Ref),
    ("НайтиПоКоду", "FindByCode", Ret::Ref),
    ("НайтиПоНаименованию", "FindByDescription", Ret::Ref),
    ("НайтиПоНомеру", "FindByNumber", Ret::Ref),
    ("Выбрать", "Select", Ret::Selection),
];

/// Методы значения объекта (объект справочника, документа, набор записей…).
const OBJECT_METHODS: &[(&str, &str)] = &[
    ("Записать", "Write"),
    ("Удалить", "Delete"),
    ("ЗаписатьИстину", "WriteTrue"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ret {
    Object,
    Ref,
    Selection,
}

/// Что известно о значении выражения в цепочке.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Val {
    /// Коллекция менеджеров (`Справочники`): класс метаданных.
    Managers(&'static str),
    /// Менеджер одного объекта (`Справочники.Валюты`).
    Object {
        class: &'static str,
        name: String,
    },
    /// Объект (`СправочникОбъект.Валюты`): реквизиты и табличные части.
    ObjectValue {
        class: &'static str,
        name: String,
    },
    /// Ссылка (`СправочникСсылка.Валюты`).
    Ref {
        class: &'static str,
        name: String,
    },
    /// Выборка объектов.
    Selection {
        class: &'static str,
        name: String,
    },
    /// Табличная часть объекта (коллекция строк).
    TabSection {
        class: &'static str,
        name: String,
        ts: String,
    },
    /// Строка табличной части: её реквизиты.
    TabRow {
        class: &'static str,
        name: String,
        ts: String,
    },
    CommonModule(String),
}

/// Метод модуля по строке заголовка: (имя, функция ли, экспорт ли, параметры в скобках).
/// Происхождение: header из constructor `src/bsl/complete.rs`.
fn header(line: &str) -> Option<(String, bool, bool, String)> {
    let t = lex::tokens(line).ok()?;
    let i = t
        .iter()
        .position(|t| matches!(t.kw, Some(Kw::Procedure | Kw::Function)))?;
    // перед словом — только директивы, аннотации и Асинх
    if t[..i]
        .iter()
        .any(|t| !(t.t == T::Directive || t.kw == Some(Kw::Async)))
    {
        return None;
    }
    let name = t
        .get(i + 1)
        .filter(|n| n.t == T::Word && n.kw.is_none())?
        .text(line)
        .to_string();
    let open = t.get(i + 2).filter(|p| p.text(line) == "(")?;
    let close = t.iter().position(|x| x.t == T::Op && x.text(line) == ")");
    let params = close
        .map(|c| line[open.end..t[c].start].to_string())
        .unwrap_or_default();
    let export = t.iter().any(|x| x.kw == Some(Kw::Export));
    Some((name, t[i].kw == Some(Kw::Function), export, params))
}

/// Параметры из текста между скобками заголовка.
fn param_names(params: &str) -> Vec<String> {
    params
        .split(',')
        .filter_map(|p| {
            let w: Vec<&str> = p.split('=').next()?.split_whitespace().collect();
            w.last().map(|s| s.to_string()).filter(|s| !s.is_empty())
        })
        .collect()
}

/// Экспортные методы модуля для подсказки после имени общего модуля или менеджера.
fn exports(text: &str) -> Vec<Item> {
    text.lines()
        .filter_map(header)
        .filter(|h| h.2)
        .map(|(name, func, _, params)| Item {
            insert: if params.trim().is_empty() {
                format!("{name}()")
            } else {
                format!("{name}(?)")
            },
            detail: format!(
                "{} {name}({})",
                if func {
                    "Функция"
                } else {
                    "Процедура"
                },
                params.trim()
            ),
            label: name,
            kind: ItemKind::Method,
        })
        .collect()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

struct Scope<'a> {
    meta: &'a dyn Meta,
    /// Текст метода от заголовка до курсора (для вывода типов локальных переменных).
    method: String,
}

impl<'a> Scope<'a> {
    /// Значение члена `name` у значения `v`.
    fn step(&self, v: &Val, name: &str) -> Option<Val> {
        match v {
            Val::Managers(class) => {
                let name = self.meta.names(class).into_iter().find(|n| is(n, name))?;
                Some(Val::Object { class, name })
            }
            Val::Object { class, name: obj } => {
                let (ru, en, ret) = MANAGER_METHODS
                    .iter()
                    .find(|(ru, en, _)| is(ru, name) || is(en, name))?;
                let _ = (ru, en);
                Some(match ret {
                    Ret::Object => Val::ObjectValue {
                        class,
                        name: obj.clone(),
                    },
                    Ret::Ref => Val::Ref {
                        class,
                        name: obj.clone(),
                    },
                    Ret::Selection => Val::Selection {
                        class,
                        name: obj.clone(),
                    },
                })
            }
            Val::ObjectValue { class, name: obj } => {
                let (kind, field) = self
                    .meta
                    .fields(class, obj, None)
                    .into_iter()
                    .find(|(_, f)| is(f, name))?;
                (kind == "TabularSection").then(|| Val::TabSection {
                    class,
                    name: obj.clone(),
                    ts: field,
                })
            }
            Val::TabSection {
                class,
                name: obj,
                ts,
            } => (is(name, "Добавить")
                || is(name, "Add")
                || is(name, "Вставить")
                || is(name, "Insert"))
            .then(|| Val::TabRow {
                class,
                name: obj.clone(),
                ts: ts.clone(),
            }),
            Val::TabRow { .. } => None,
            Val::Ref { class, name: obj } => (is(name, "ПолучитьОбъект") || is(name, "GetObject"))
                .then(|| Val::ObjectValue {
                    class,
                    name: obj.clone(),
                }),
            Val::Selection { .. } => None,
            Val::CommonModule(_) => None,
        }
    }

    /// Значение имени в начале цепочки: локальная переменная, общий модуль, коллекция менеджеров.
    fn head(&self, name: &str, depth: u32) -> Option<Val> {
        if let Some(v) = self.local(name, depth) {
            return Some(v);
        }
        if self.meta.names("CommonModule").iter().any(|m| is(m, name)) {
            return Some(Val::CommonModule(name.to_string()));
        }
        collection_class(name).map(Val::Managers)
    }

    /// Тип локальной переменной по последнему присваиванию в методе до курсора.
    fn local(&self, name: &str, depth: u32) -> Option<Val> {
        if depth > 4 {
            return None;
        }
        let toks = lex::tokens(&self.method).ok()?;
        let src = self.method.as_str();
        let n = name.to_lowercase();
        let mut found = None;
        for (i, t) in toks.iter().enumerate() {
            if t.t != T::Word || t.text(src).to_lowercase() != n {
                continue;
            }
            let starts = i == 0
                || toks[i - 1].text(src) == ";"
                || toks[i - 1].kw.is_some()
                || toks[i - 1].line < t.line;
            if starts
                && toks
                    .get(i + 1)
                    .is_some_and(|x| x.t == T::Op && x.text(src) == "=")
            {
                let end = toks[i + 2..]
                    .iter()
                    .position(|x| x.text(src) == ";" || x.t == T::Eof)
                    .map_or(toks.len(), |e| e + i + 2);
                found = Some((i + 2..end, false));
            }
            if i > 0
                && toks[i - 1].kw == Some(Kw::Each)
                && toks.get(i + 1).is_some_and(|x| x.kw == Some(Kw::In))
            {
                let end = toks[i + 2..]
                    .iter()
                    .position(|x| x.kw == Some(Kw::Do) || x.t == T::Eof)
                    .map_or(toks.len(), |e| e + i + 2);
                found = Some((i + 2..end, true));
            }
        }
        let (r, each) = found?;
        let v = self.eval(src, &toks[r], depth + 1)?;
        if each {
            self.element(&v)
        } else {
            Some(v)
        }
    }

    /// Элемент коллекции при обходе `Для Каждого`: строка таблицы значений, объект выборки,
    /// строка табличной части; у прочих — по `Добавить`/`Получить`.
    fn element(&self, v: &Val) -> Option<Val> {
        match v {
            Val::Selection { class, name } => Some(Val::ObjectValue {
                class,
                name: name.clone(),
            }),
            _ => ["Добавить", "Получить"]
                .iter()
                .find_map(|m| self.step(v, m)),
        }
    }

    /// Значение выражения из лексем: цепочка `Имя(…).Член[…]…`.
    fn eval(&self, src: &str, t: &[Token], depth: u32) -> Option<Val> {
        let segs = chain(src, t)?;
        self.walk(&segs, depth)
    }

    fn walk(&self, segs: &[(String, bool)], depth: u32) -> Option<Val> {
        let (first, rest) = segs.split_first()?;
        let mut v = self.head(&first.0, depth)?;
        for (name, _) in rest {
            v = self.step(&v, name)?;
        }
        Some(v)
    }

    fn items(&self, v: &Val) -> Vec<Item> {
        match v {
            Val::Managers(class) => self
                .meta
                .names(class)
                .into_iter()
                .map(|n| Item {
                    insert: n.clone(),
                    label: n,
                    kind: ItemKind::Object,
                    detail: self.meta.title(class),
                })
                .collect(),
            Val::Object { class, name } => {
                let mut out: Vec<Item> = self
                    .meta
                    .module(class, name, "ManagerModule")
                    .map(|t| exports(&t))
                    .unwrap_or_default();
                out.extend(MANAGER_METHODS.iter().map(|(ru, en, _)| Item {
                    insert: ru.to_string(),
                    label: ru.to_string(),
                    kind: ItemKind::Method,
                    detail: format!("{en} — метод менеджера"),
                }));
                out
            }
            Val::ObjectValue { class, name } => {
                let mut out: Vec<Item> = self
                    .meta
                    .fields(class, name, None)
                    .into_iter()
                    .map(|(kind, n)| Item {
                        insert: n.clone(),
                        label: n,
                        kind: ItemKind::Property,
                        detail: kind_ru(kind).to_string(),
                    })
                    .collect();
                out.extend(OBJECT_METHODS.iter().map(|(ru, en)| Item {
                    insert: ru.to_string(),
                    label: ru.to_string(),
                    kind: ItemKind::Method,
                    detail: format!("{en} — метод объекта"),
                }));
                out
            }
            Val::Ref { .. } => vec![Item {
                label: "ПолучитьОбъект".into(),
                insert: "ПолучитьОбъект()".into(),
                kind: ItemKind::Method,
                detail: "GetObject — метод ссылки".into(),
            }],
            Val::Selection { class, name } => self
                .meta
                .fields(class, name, None)
                .into_iter()
                .filter(|(kind, _)| *kind == "Attribute")
                .map(|(_, n)| Item {
                    insert: n.clone(),
                    label: n,
                    kind: ItemKind::Property,
                    detail: "Поле выборки".into(),
                })
                .collect(),
            Val::TabSection { class, name, ts } => {
                let mut out: Vec<Item> = [
                    ("Добавить", "Add"),
                    ("Вставить", "Insert"),
                    ("Удалить", "Delete"),
                    ("Очистить", "Clear"),
                    ("Количество", "Count"),
                ]
                .iter()
                .map(|(ru, en)| Item {
                    label: ru.to_string(),
                    insert: format!("{ru}()"),
                    kind: ItemKind::Method,
                    detail: format!("{en} — метод табличной части"),
                })
                .collect();
                out.extend(
                    self.meta
                        .fields(class, name, Some(ts))
                        .into_iter()
                        .map(|(_, n)| Item {
                            insert: n.clone(),
                            label: n,
                            kind: ItemKind::Property,
                            detail: "Колонка табличной части".into(),
                        }),
                );
                out
            }
            Val::TabRow { class, name, ts } => self
                .meta
                .fields(class, name, Some(ts))
                .into_iter()
                .map(|(_, n)| Item {
                    insert: n.clone(),
                    label: n,
                    kind: ItemKind::Property,
                    detail: "Реквизит табличной части".into(),
                })
                .collect(),
            Val::CommonModule(name) => self
                .meta
                .module("CommonModule", name, "Module")
                .map(|t| exports(&t))
                .unwrap_or_default(),
        }
    }
}

/// Вид подчиненного объекта по-русски (как в дереве метаданных).
fn kind_ru(kind: &str) -> &str {
    match kind {
        "Attribute" => "Реквизит",
        "TabularSection" => "Табличная часть",
        "Dimension" => "Измерение",
        "Resource" => "Ресурс",
        "EnumValue" => "Значение перечисления",
        k => k,
    }
}

/// Цепочка `Имя(…).Член[…].Член` из лексем. Происхождение: chain из constructor `src/bsl/complete.rs`.
fn chain(src: &str, t: &[Token]) -> Option<Vec<(String, bool)>> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut i = 0;
    while i < t.len() && t[i].t != T::Eof {
        if t[i].t != T::Word {
            return None;
        }
        out.push((t[i].text(src).to_string(), false));
        i += 1;
        // скобки и индексы после имени
        while i < t.len() && matches!(t[i].text(src), "(" | "[") && t[i].t == T::Op {
            let (open, close) = if t[i].text(src) == "(" {
                ("(", ")")
            } else {
                ("[", "]")
            };
            let mut d = 0;
            loop {
                match (t.get(i)?.t, t[i].text(src)) {
                    (T::Op, s) if s == open => d += 1,
                    (T::Op, s) if s == close => d -= 1,
                    _ => {}
                }
                i += 1;
                if d == 0 {
                    break;
                }
            }
        }
        if i < t.len() && t[i].t == T::Op && t[i].text(src) == "." {
            i += 1;
        } else {
            break;
        }
    }
    (i >= t.len() || t[i].t == T::Eof).then_some(out)
}

/// Метод модуля от последнего заголовка до курсора и строка заголовка.
fn scope_at<'a>(
    meta: &'a dyn Meta,
    lines: &[String],
    line: usize,
    before: &str,
) -> (Scope<'a>, Option<usize>) {
    let head_line = (0..=line.min(lines.len().saturating_sub(1)))
        .rev()
        .find(|&i| header(&lines[i]).is_some());
    let mut method = head_line
        .map(|h| lines[(h + 1).min(line)..line].join("\n"))
        .unwrap_or_else(|| lines[..line].join("\n"));
    method.push('\n');
    method.push_str(before);
    (Scope { meta, method }, head_line)
}

/// Подсказка в месте курсора (`line` от 0, `col` — знак строки): варианты, отобранные по началу
/// слова без учёта регистра.
pub fn complete(meta: &dyn Meta, lines: &[String], line: usize, col: usize) -> Vec<Item> {
    let text: Vec<char> = lines
        .get(line)
        .map(|l| l.chars().collect())
        .unwrap_or_default();
    let col = col.min(text.len());
    let mut start = col;
    while start > 0 && is_word(text[start - 1]) {
        start -= 1;
    }
    let prefix: String = text[start..col].iter().collect::<String>().to_lowercase();
    let before: String = text[..start].iter().collect();
    let (scope, head_line) = scope_at(meta, lines, line, &before);
    let method = scope.method.clone();
    let mut items = Vec::new();
    let trimmed = before.trim_end();
    if trimmed.ends_with('.') {
        // цепочка перед точкой — лексемы строки от последнего разделителя
        let expr = &trimmed[..trimmed.len() - 1];
        if let Ok(toks) = lex::tokens(expr) {
            let toks: Vec<Token> = toks.into_iter().filter(|t| t.t != T::Eof).collect();
            for s in 0..toks.len() {
                if let Some(segs) = chain(expr, &toks[s..]) {
                    if let Some(v) = scope.walk(&segs, 0) {
                        items = scope.items(&v);
                    }
                    break;
                }
            }
        }
    } else {
        items.extend(KEYWORDS.iter().map(|k| Item {
            label: k.to_string(),
            insert: k.to_string(),
            kind: ItemKind::Keyword,
            detail: String::new(),
        }));
        items.extend(
            lines
                .iter()
                .filter_map(|l| header(l))
                .map(|(name, func, _, params)| Item {
                    insert: if params.trim().is_empty() {
                        format!("{name}()")
                    } else {
                        format!("{name}(?)")
                    },
                    detail: format!(
                        "{} {name}({})",
                        if func {
                            "Функция"
                        } else {
                            "Процедура"
                        },
                        params.trim()
                    ),
                    label: name,
                    kind: ItemKind::Method,
                }),
        );
        items.extend(meta.names("CommonModule").into_iter().map(|n| Item {
            insert: n.clone(),
            label: n,
            kind: ItemKind::Module,
            detail: "Общий модуль".into(),
        }));
        items.extend(
            RU_MANAGERS
                .iter()
                .chain(MANAGERS.iter())
                .map(|(name, _)| Item {
                    label: name.to_string(),
                    insert: name.to_string(),
                    kind: ItemKind::Object,
                    detail: "Коллекция менеджеров".into(),
                }),
        );
        // переменные: параметры метода, Перем и присваивания в методе до курсора, Перем модуля
        let mut vars: Vec<String> = head_line
            .and_then(|h| header(&lines[h]))
            .map(|h| param_names(&h.3))
            .unwrap_or_default();
        if let Ok(toks) = lex::tokens(&method) {
            for (i, t) in toks.iter().enumerate() {
                let assign = toks
                    .get(i + 1)
                    .is_some_and(|x| x.t == T::Op && x.text(&method) == "=")
                    && (i == 0
                        || toks[i - 1].text(&method) == ";"
                        || toks[i - 1].kw.is_some()
                        || toks[i - 1].line < t.line);
                let declared = i > 0 && matches!(toks[i - 1].kw, Some(Kw::Var | Kw::Each))
                    || (i > 0
                        && toks[i - 1].text(&method) == ","
                        && toks[..i]
                            .iter()
                            .rev()
                            .find(|x| x.kw.is_some())
                            .is_some_and(|x| x.kw == Some(Kw::Var)));
                if t.t == T::Word && t.kw.is_none() && (assign || declared) {
                    vars.push(t.text(&method).to_string());
                }
            }
        }
        for l in lines {
            if let Ok(t) = lex::tokens(l) {
                if t.first()
                    .is_some_and(|x| x.kw == Some(Kw::Var) || x.t == T::Directive)
                    && t.iter().any(|x| x.kw == Some(Kw::Var))
                {
                    vars.extend(
                        t.iter()
                            .filter(|x| x.t == T::Word && x.kw.is_none())
                            .map(|x| x.text(l).to_string()),
                    );
                }
            }
        }
        items.extend(vars.into_iter().map(|v| Item {
            insert: v.clone(),
            label: v,
            kind: ItemKind::Var,
            detail: String::new(),
        }));
    }
    items.retain(|i| i.label.to_lowercase().starts_with(&prefix));
    items.sort_by(|a, b| {
        a.label
            .to_lowercase()
            .cmp(&b.label.to_lowercase())
            .then(a.kind.cmp(&b.kind))
    });
    items.dedup_by(|a, b| a.label.to_lowercase() == b.label.to_lowercase());
    items
}

/// Куда ведёт «Перейти к определению» (F12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Строка этого модуля (от 0): заголовок метода, `Перем`, параметр или первое присваивание.
    Line(usize),
    /// Экспортный метод модуля объекта: класс, имя объекта, роль модуля, имя метода.
    Method {
        class: &'static str,
        name: String,
        role: &'static str,
        method: String,
    },
    /// Имя объекта метаданных (второй квалификатор коллекции): класс, имя, идентификатор дерева.
    Metadata {
        class: &'static str,
        name: String,
        id: String,
    },
}

/// Строка заголовка метода `name` в тексте модуля (от 0).
pub fn method_line<S: AsRef<str>>(lines: &[S], name: &str) -> Option<usize> {
    lines
        .iter()
        .position(|l| header(l.as_ref()).is_some_and(|h| is(&h.0, name)))
}

/// Определение слова под курсором: метод или переменная модуля, параметр или переменная метода,
/// экспортный метод общего модуля (`Модуль.Метод`) или менеджера (`Справочники.Имя.Метод`),
/// имя объекта метаданных (`Справочники.Имя`). Недостоверные цели не выдумываются.
pub fn definition(meta: &dyn Meta, lines: &[String], line: usize, col: usize) -> Option<Target> {
    let text: Vec<char> = lines.get(line)?.chars().collect();
    let (mut a, mut b) = (col.min(text.len()), col.min(text.len()));
    while a > 0 && is_word(text[a - 1]) {
        a -= 1;
    }
    while b < text.len() && is_word(text[b]) {
        b += 1;
    }
    let word: String = text[a..b].iter().collect();
    if word.is_empty() {
        return None;
    }
    // квалификаторы перед словом: «А.Б.слово» → [А, Б]
    let mut quals = Vec::new();
    let mut i = a;
    while i > 0 && text[i - 1] == '.' {
        let e = i - 1;
        let mut s = e;
        while s > 0 && is_word(text[s - 1]) {
            s -= 1;
        }
        if s == e {
            break;
        }
        quals.insert(0, text[s..e].iter().collect::<String>());
        i = s;
    }
    match quals.as_slice() {
        [] => {}
        [m] if meta.names("CommonModule").iter().any(|n| is(n, m)) => {
            let name = meta.names("CommonModule").into_iter().find(|n| is(n, m))?;
            return Some(Target::Method {
                class: "CommonModule",
                name,
                role: "Module",
                method: word,
            });
        }
        [coll] if collection_class(coll).is_some() => {
            // `Коллекция.Имя` — определение самого объекта метаданных.
            let class = collection_class(coll)?;
            let id = meta.id(class, &word)?;
            return Some(Target::Metadata {
                class,
                name: word,
                id,
            });
        }
        [coll, obj] => {
            let class = collection_class(coll)?;
            let name = meta.names(class).into_iter().find(|n| is(n, obj))?;
            return Some(Target::Method {
                class,
                name,
                role: "ManagerModule",
                method: word,
            });
        }
        _ => return None,
    }
    if let Some(l) = method_line(lines, &word) {
        return Some(Target::Line(l));
    }
    // переменная: параметр или первое объявление/присваивание в методе, затем Перем модуля
    let head = (0..=line.min(lines.len().saturating_sub(1)))
        .rev()
        .find(|&i| header(&lines[i]).is_some());
    let declares = |l: &str| -> bool {
        let Ok(t) = lex::tokens(l) else { return false };
        t.iter().enumerate().any(|(i, x)| {
            x.t == T::Word
                && is(x.text(l), &word)
                && ((i == 0
                    || t[i - 1].text(l) == ";"
                    || t[i - 1].kw.is_some_and(|k| k != Kw::New))
                    && t.get(i + 1)
                        .is_some_and(|n| n.text(l) == "=" || n.text(l) == ";" || n.text(l) == ",")
                    || t[..i].iter().any(|k| k.kw == Some(Kw::Var)))
        })
    };
    if let Some(h) = head {
        if header(&lines[h]).is_some_and(|x| param_names(&x.3).iter().any(|p| is(p, &word))) {
            return Some(Target::Line(h));
        }
        if let Some(l) = (h + 1..=line).find(|&i| declares(&lines[i])) {
            return Some(Target::Line(l));
        }
    }
    let first_method = (0..lines.len())
        .find(|&i| header(&lines[i]).is_some())
        .unwrap_or(lines.len());
    (0..first_method)
        .find(|&i| lines[i].trim_start().to_lowercase().starts_with("перем") && declares(&lines[i]))
        .map(Target::Line)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Метаданные синтетики: справочник Валюты (реквизит Курс, табличная часть Курсы с реквизитом Период),
    /// общий модуль ОбщегоНазначения с экспортной функцией.
    struct Fixture;
    impl Meta for Fixture {
        fn names(&self, class: &str) -> Vec<String> {
            match class {
                "Catalog" => vec!["Валюты".into()],
                "CommonModule" => vec!["ОбщегоНазначения".into()],
                _ => Vec::new(),
            }
        }
        fn module(&self, class: &str, name: &str, role: &str) -> Option<String> {
            (class == "CommonModule" && name == "ОбщегоНазначения" && role == "Module")
                .then(|| "Функция ЗначениеРеквизита(Ссылка, Имя) Экспорт\nКонецФункции\nПроцедура Внутренняя()\nКонецПроцедуры".into())
        }
        fn fields(&self, class: &str, name: &str, ts: Option<&str>) -> Vec<(&'static str, String)> {
            match (class, name, ts) {
                ("Catalog", "Валюты", None) => vec![
                    ("Attribute", "Курс".into()),
                    ("TabularSection", "Курсы".into()),
                ],
                ("Catalog", "Валюты", Some("Курсы")) => {
                    vec![("Attribute", "Период".into())]
                }
                _ => Vec::new(),
            }
        }
        fn id(&self, class: &str, name: &str) -> Option<String> {
            (class == "Catalog" && name == "Валюты").then(|| "uuid-валюты".into())
        }
        fn title(&self, class: &str) -> String {
            match class {
                "Catalog" => "Справочники".into(),
                _ => class.into(),
            }
        }
    }

    fn at(text: &str) -> Vec<String> {
        let lines: Vec<String> = text.split('\n').map(String::from).collect();
        let line = lines.len() - 1;
        let col = lines[line].chars().count();
        complete(&Fixture, &lines, line, col)
            .into_iter()
            .map(|i| i.label)
            .collect()
    }

    #[test]
    fn manager_collection_and_objects() {
        assert!(at("Процедура П()\n\tСправочники.").contains(&"Валюты".to_string()));
        assert!(at("Процедура П()\n\tCatalogs.").contains(&"Валюты".to_string()));
        // незавершённый ввод: префикс отбирает
        assert_eq!(at("Процедура П()\n\tСправочники.Ва"), ["Валюты"]);
        // неизвестная коллекция — пусто
        assert_eq!(at("Процедура П()\n\tНеКоллекция."), Vec::<String>::new());
    }

    #[test]
    fn common_module_exports() {
        assert_eq!(
            at("Процедура П()\n\tОбщегоНазначения."),
            ["ЗначениеРеквизита"]
        );
        // внутренний метод общего модуля не предлагается
        assert!(!at("Процедура П()\n\tОбщегоНазначения.").contains(&"Внутренняя".to_string()));
    }

    #[test]
    fn known_object_fields_and_ts() {
        // менеджер: собственные методы + экспорт менеджера модуля
        let items = at("Процедура П()\n\tСправочники.Валюты.");
        assert!(items.contains(&"СоздатьЭлемент".to_string()), "{items:?}");
        // объект через создание: реквизиты и табличные части
        let items = at("Процедура П()\n\tО = Справочники.Валюты.СоздатьЭлемент();\n\tО.");
        assert!(items.contains(&"Курс".to_string()), "{items:?}");
        assert!(items.contains(&"Курсы".to_string()), "{items:?}");
        // строка табличной части: её реквизиты
        let items =
            at("Процедура П()\n\tС = Справочники.Валюты.СоздатьЭлемент().Курсы.Добавить();\n\tС.");
        assert!(items.contains(&"Период".to_string()), "{items:?}");
        // выборка через Для Каждого
        let items =
            at("Процедура П()\n\tДля Каждого С Из Справочники.Валюты.Выбрать() Цикл\n\t\tС.");
        assert!(items.contains(&"Курс".to_string()), "{items:?}");
        // ссылка: ПолучитьОбъект
        let items = at("Процедура П()\n\tСправочники.Валюты.ПустаяСсылка().");
        assert!(items.contains(&"ПолучитьОбъект".to_string()), "{items:?}");
        // у реквизита дальше ничего не известно — пусто, без выдумки
        assert_eq!(
            at("Процедура П()\n\tО = Справочники.Валюты.СоздатьЭлемент();\n\tО.Курс."),
            Vec::<String>::new()
        );
    }

    #[test]
    fn plain_scope_keywords_methods_vars() {
        let names = at("Перем Модульная;\nПроцедура П(Знач Пар1, Пар2 = 1)\n\tЛок = 1;\n\tДля Каждого Эл Из М Цикл\n\t");
        for n in [
            "Модульная",
            "Пар1",
            "Пар2",
            "Лок",
            "Эл",
            "П",
            "ОбщегоНазначения",
            "Справочники",
            "Если",
        ] {
            assert!(names.contains(&n.to_string()), "{n}: {names:?}");
        }
    }

    fn def(text: &str, line: usize, col: usize) -> Option<Target> {
        let lines: Vec<String> = text.split('\n').map(String::from).collect();
        definition(&Fixture, &lines, line, col)
    }

    #[test]
    fn definition_local_and_module() {
        // локальная процедура — её строка
        assert_eq!(
            def(
                "Процедура А()\nКонецПроцедуры\nПроцедура Б()\n\tА();\nКонецПроцедуры",
                3,
                2
            ),
            Some(Target::Line(0))
        );
        // параметр — строка заголовка метода
        assert_eq!(
            def("Процедура П(Знач Пар)\n\tПар = 1;\nКонецПроцедуры", 1, 2),
            Some(Target::Line(0))
        );
        // присваивание в методе — первое определение переменной
        assert_eq!(
            def("Перем М;\nПроцедура П()\n\tМ = 1;\nКонецПроцедуры", 2, 2),
            Some(Target::Line(2))
        );
        // переменная модуля без присваивания в методе — её Перем
        assert_eq!(
            def("Перем М;\nПроцедура П()\n\tВ = М;\nКонецПроцедуры", 2, 6),
            Some(Target::Line(0))
        );
        // непонятная цель — пусто
        assert_eq!(
            def("Процедура П()\n\tНичего();\nКонецПроцедуры", 1, 3),
            None
        );
    }

    #[test]
    fn definition_cross_module_and_metadata() {
        // метод общего модуля
        assert_eq!(
            def(
                "Процедура П()\n\tОбщегоНазначения.ЗначениеРеквизита(А, Б);\nКонецПроцедуры",
                1,
                22
            ),
            Some(Target::Method {
                class: "CommonModule",
                name: "ОбщегоНазначения".into(),
                role: "Module",
                method: "ЗначениеРеквизита".into()
            })
        );
        // метод менеджера объекта (курсор на имени метода)
        assert_eq!(
            def(
                "Процедура П()\n\tСправочники.Валюты.НайтиПоКоду(\"1\");\nКонецПроцедуры",
                1,
                22
            ),
            Some(Target::Method {
                class: "Catalog",
                name: "Валюты".into(),
                role: "ManagerModule",
                method: "НайтиПоКоду".into()
            })
        );
        // имя объекта метаданных — к самому объекту
        assert_eq!(
            def(
                "Процедура П()\n\tСправочники.Валюты.ПустаяСсылка();\nКонецПроцедуры",
                1,
                14
            ),
            Some(Target::Metadata {
                class: "Catalog",
                name: "Валюты".into(),
                id: "uuid-валюты".into()
            })
        );
        // неизвестный объект коллекции — пусто
        assert_eq!(
            def(
                "Процедура П()\n\tСправочники.НеТо.ПустаяСсылка();\nКонецПроцедуры",
                1,
                14
            ),
            None
        );
    }
}
