//! Встроенный язык 1С: лексемы, разбор модуля в дерево (переменные, методы, операторы, выражения), ошибки синтаксиса.
//!
//! Происхождение: перенесено без изменений логики из constructor `src/bsl/mod.rs` (собственный код проекта).
//! Правки переноса: убраны модули `complete`/`ctx`/`undef` с платформенными зависимостями (HBK-контекст,
//! constructor Config); `check_all` не принимает множество известных имён — проверка «переменная не определена»
//! без полного контекста платформы ложно срабатывала бы на её глобальные имена. Проверка обращений к
//! несуществующим объектам известных коллекций метаданных живёт в `analysis.rs`.
//! `allow(dead_code)`: полное AST/разбор нужны частично (полные возможности — в исходнике constructor).

#![allow(dead_code)]

pub mod analysis;
pub mod lex;

use lex::{Kw, Token, T};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub line: u32,
    pub col: u32,
    pub msg: String,
}

/// Место имени в тексте: строка и колонка от 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct At {
    pub line: u32,
    pub col: u32,
}

#[derive(Clone, Debug)]
pub struct Var {
    pub name: String,
    pub at: At,
    pub export: bool,
    pub directives: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub at: At,
    pub val: bool,
    pub default: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct Method {
    pub name: String,
    pub at: At,
    pub func: bool,
    pub asynch: bool,
    pub export: bool,
    pub directives: Vec<String>,
    pub params: Vec<Param>,
    pub body: Vec<St>,
    /// Строка `КонецПроцедуры`/`КонецФункции`.
    pub end_line: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Module {
    pub vars: Vec<Var>,
    pub methods: Vec<Method>,
    pub body: Vec<St>,
}

#[derive(Clone, Debug)]
pub struct St {
    pub line: u32,
    pub s: Stmt,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Assign(Expr, Expr),
    Call(Expr),
    If(Vec<(Expr, Vec<St>)>, Option<Vec<St>>),
    While(Expr, Vec<St>),
    For(String, At, Expr, Expr, Vec<St>),
    ForEach(String, At, Expr, Vec<St>),
    Try(Vec<St>, Vec<St>),
    Return(Option<Expr>),
    Continue,
    Break,
    Raise(Vec<Option<Expr>>),
    Execute(Expr),
    Goto(String),
    Label(String),
    AddHandler(Expr, Expr),
    RemoveHandler(Expr, Expr),
    Var(Vec<Var>),
    Await(Expr),
}

#[derive(Clone, Debug)]
pub enum Expr {
    Num(String),
    Str(String),
    Date(String),
    Bool(bool),
    Undefined,
    Null,
    Name(String, At),
    Member(Box<Expr>, String, At),
    Index(Box<Expr>, Box<Expr>),
    /// Вызов: аргументы и место каждого (начало лексемы аргумента).
    Call(Box<Expr>, Vec<Option<Expr>>, Vec<At>),
    /// `Новый Тип(…)` или `Новый(Тип, …)` (тогда имени нет).
    New(Option<String>, Vec<Option<Expr>>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Await(Box<Expr>),
}

/// Контекст компиляции, по которому выбираются ветки `#Если`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ctx {
    Server,
    ThinClient,
}

/// Разбор модуля; при ошибке — первая ошибка. С `#Если` модуль проверяется в контекстах сервера и тонкого клиента,
/// как `/CheckModules -Server -ThinClient`; дерево — по всем веткам подряд, если они так разбираются, иначе по серверу.
// shortcut: только первая ошибка модуля — восстановление после ошибки, когда понадобятся все ошибки сразу.
pub fn parse(src: &str) -> Result<Module, Error> {
    parse_in(src, Site::Other)
}

/// Разбор модуля с местом компиляции: проверки, зависящие от вида модуля и директив.
pub fn parse_in(src: &str, site: Site) -> Result<Module, Error> {
    let toks = lex::tokens(src)?;
    let run = |ctx| -> Result<Module, Error> {
        let t = strip_preproc(src, &toks, ctx)?;
        let server = ctx != Some(Ctx::ThinClient);
        let m = Parser {
            src,
            t,
            i: 0,
            func: None,
            asynch: false,
            loops: 0,
            excepts: 0,
            vars_ok: true,
            site,
            server,
            ..Default::default()
        }
        .module()?;
        check_calls(&m)?;
        Ok(m)
    };
    let has_if = toks
        .iter()
        .any(|t| t.t == T::Preproc && matches!(preproc_word(src, t).as_str(), "если" | "if"));
    if !has_if {
        return run(None);
    }
    let server = run(Some(Ctx::Server))?;
    run(Some(Ctx::ThinClient))?;
    Ok(run(None).unwrap_or(server))
}

/// Все ошибки модуля, как `/CheckModules`: синтаксис — с продолжением со следующего метода, в контекстах сервера и
/// тонкого клиента (при `#Если`), вызовы методов модуля, обращения к несуществующим объектам известных коллекций
/// метаданных (`exists` — регистронезависимая проверка имени объекта класса; `None` — без неё). По тексту, без
/// повторов. Проверки имён, требующие полного контекста платформы (неопределённые переменные), не выполняются:
/// без него они недостоверны.
pub fn check_all(
    src: &str,
    site: Site,
    mut exists: Option<&mut dyn FnMut(&str, &str) -> bool>,
) -> Vec<Error> {
    let toks = match lex::tokens(src) {
        Ok(t) => t,
        Err(e) => return vec![e],
    };
    let has_if = toks
        .iter()
        .any(|t| t.t == T::Preproc && matches!(preproc_word(src, t).as_str(), "если" | "if"));
    let mut out: Vec<Error> = Vec::new();
    // модули объекта, менеджера и т. п. на тонком клиенте не компилируются: `#Если Клиент` в них не проверяется
    let passes = match (has_if, site) {
        (false, _) => vec![None],
        (true, Site::Server) => vec![Some(Ctx::Server)],
        (true, _) => vec![Some(Ctx::Server), Some(Ctx::ThinClient)],
    };
    for ctx in passes {
        let t = match strip_preproc(src, &toks, ctx) {
            Ok(t) => t,
            Err(e) => {
                out.push(e);
                continue;
            }
        };
        let server = ctx != Some(Ctx::ThinClient);
        let mut p = Parser {
            src,
            t,
            i: 0,
            func: None,
            asynch: false,
            loops: 0,
            excepts: 0,
            vars_ok: true,
            site,
            server,
            recover: true,
            ..Default::default()
        };
        let m = p.module();
        out.append(&mut p.errors);
        match m {
            Ok(m) => {
                out.extend(calls(&m));
                if let Some(exists) = exists.as_deref_mut() {
                    out.extend(member_errors(&m, exists));
                }
            }
            Err(e) => out.push(e),
        }
    }
    out.sort_by_key(|e| (e.line, e.col));
    out.dedup_by(|a, b| (a.line, a.col, &a.msg) == (b.line, b.col, &b.msg));
    out
}

/// Известные коллекции менеджеров (нижний регистр) → класс объектов метаданных. Здесь только коллекции,
/// состав которых достоверно известен из самой конфигурации; динамические контексты платформы не проверяются.
fn collection_class(name: &str) -> Option<&'static str> {
    Some(match name.to_lowercase().as_str() {
        "справочники" | "catalogs" => "Catalog",
        "документы" | "documents" => "Document",
        "перечисления" | "enums" => "Enum",
        "отчеты" | "reports" => "Report",
        "обработки" | "dataprocessors" => "DataProcessor",
        "планывидовхарактеристик" | "chartsofcharacteristictypes" => {
            "ChartOfCharacteristicTypes"
        }
        "планысчетов" | "chartsofaccounts" => "ChartOfAccounts",
        "планывидоврасчета" | "chartsofcalculationtypes" => {
            "ChartOfCalculationTypes"
        }
        "регистрысведений" | "informationregisters" => "InformationRegister",
        "регистрынакопления" | "accumulationregisters" => "AccumulationRegister",
        "регистрыбухгалтерии" | "accountingregisters" => "AccountingRegister",
        "регистрырасчета" | "calculationregisters" => "CalculationRegister",
        "бизнеспроцессы" | "businessprocesses" => "BusinessProcess",
        "задачи" | "tasks" => "Task",
        "журналыдокументов" | "documentjournals" => "DocumentJournal",
        "константы" | "constants" => "Constant",
        "общиемодули" | "commonmodules" => "CommonModule",
        "роли" | "roles" => "Role",
        "планыобмена" | "exchangeplans" => "ExchangePlan",
        "параметрысессии" | "sessionparameters" => "SessionParameter",
        "регламентныезадания" | "scheduledjobs" => "ScheduledJob",
        "внешниеисточникиданных" | "externaldatasources" => {
            "ExternalDataSource"
        }
        _ => return None,
    })
}

/// Обращения `Коллекция.Имя`, где коллекция известна, а объекта с таким именем нет: «Объект метаданных не найден».
/// Чужие и динамические коллекции не проверяются. `exists(class, name)` — регистронезависимый поиск объекта.
pub fn member_errors(m: &Module, exists: &mut dyn FnMut(&str, &str) -> bool) -> Vec<Error> {
    let mut w = Coll {
        exists,
        out: Vec::new(),
    };
    for f in &m.methods {
        for p in &f.params {
            if let Some(d) = &p.default {
                w.expr(d);
            }
        }
        w.block(&f.body);
    }
    w.block(&m.body);
    w.out
}

struct Coll<'a> {
    exists: &'a mut dyn FnMut(&str, &str) -> bool,
    out: Vec<Error>,
}

impl Coll<'_> {
    fn name(&mut self, coll: &str, n: &str, at: At) {
        if let Some(class) = collection_class(coll) {
            if !(self.exists)(class, n) {
                self.out.push(Error {
                    line: at.line,
                    col: at.col,
                    msg: format!("Объект метаданных не найден ({coll}.{n})"),
                });
            }
        }
    }
    fn block(&mut self, b: &[St]) {
        for s in b {
            self.st(s);
        }
    }
    fn st(&mut self, s: &St) {
        match &s.s {
            Stmt::Assign(l, r) => {
                self.expr(l);
                self.expr(r);
            }
            Stmt::Call(e) | Stmt::Execute(e) | Stmt::Await(e) | Stmt::Return(Some(e)) => {
                self.expr(e)
            }
            Stmt::If(arms, other) => {
                for (c, b) in arms {
                    self.expr(c);
                    self.block(b);
                }
                if let Some(b) = other {
                    self.block(b);
                }
            }
            Stmt::While(c, b) => {
                self.expr(c);
                self.block(b);
            }
            Stmt::For(_, _, from, to, b) => {
                self.expr(from);
                self.expr(to);
                self.block(b);
            }
            Stmt::ForEach(_, _, from, b) => {
                self.expr(from);
                self.block(b);
            }
            Stmt::Try(a, b) => {
                self.block(a);
                self.block(b);
            }
            Stmt::Raise(args) => args.iter().flatten().for_each(|e| self.expr(e)),
            Stmt::AddHandler(a, b) | Stmt::RemoveHandler(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            Stmt::Var(_)
            | Stmt::Return(None)
            | Stmt::Continue
            | Stmt::Break
            | Stmt::Goto(_)
            | Stmt::Label(_) => {}
        }
    }
    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Name(..) => {}
            Expr::Member(b, n, at) => {
                if let Expr::Name(coll, _) = &**b {
                    self.name(coll, n, *at);
                }
                self.expr(b);
            }
            Expr::Index(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Call(f, args, _) => {
                self.expr(f);
                args.iter().flatten().for_each(|a| self.expr(a));
            }
            Expr::New(_, args) => args.iter().flatten().for_each(|a| self.expr(a)),
            Expr::Unary(_, a) | Expr::Await(a) => self.expr(a),
            Expr::Binary(_, a, b) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Ternary(a, b, c) => {
                self.expr(a);
                self.expr(b);
                self.expr(c);
            }
            Expr::Num(_)
            | Expr::Str(_)
            | Expr::Date(_)
            | Expr::Bool(_)
            | Expr::Undefined
            | Expr::Null => {}
        }
    }
}

/// Ключевое слово в сообщении, как у платформы: `'КонецЕсли' ('EndIf')`.
fn kw_name(k: Kw) -> String {
    let (ru, en) = match k {
        Kw::EndIf => ("КонецЕсли", "EndIf"),
        Kw::Then => ("Тогда", "Then"),
        Kw::Do => ("Цикл", "Do"),
        Kw::EndDo => ("КонецЦикла", "EndDo"),
        Kw::To => ("По", "To"),
        Kw::In => ("Из", "In"),
        Kw::Except => ("Исключение", "Except"),
        Kw::EndTry => ("КонецПопытки", "EndTry"),
        Kw::EndProcedure => ("КонецПроцедуры", "EndProcedure"),
        Kw::EndFunction => ("КонецФункции", "EndFunction"),
        _ => ("?", "?"),
    };
    format!("'{ru}' ('{en}')")
}

/// Первое слово инструкции препроцессора в нижнем регистре.
fn preproc_word(src: &str, t: &Token) -> String {
    t.text(src)[1..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<String>()
        .to_lowercase()
}

/// Условие `#Если …Тогда` в контексте: символы контекста, `И`/`Или`/`Не`, скобки; незнакомый символ — ложь.
fn preproc_cond(line: &str, ctx: Ctx) -> bool {
    let mut words = Vec::new();
    let mut w = String::new();
    for c in line.chars().chain([' ']) {
        if c.is_alphanumeric() || c == '_' {
            w.extend(c.to_lowercase());
        } else {
            if !w.is_empty() {
                words.push(std::mem::take(&mut w));
            }
            if c == '(' || c == ')' {
                words.push(c.to_string());
            }
        }
    }
    // Слова после `#Если`/`#ИначеЕсли` до `Тогда`.
    let end = words
        .iter()
        .position(|w| w == "тогда" || w == "then")
        .unwrap_or(words.len());
    let words = &words[1.min(end)..end];
    let on = |s: &str| match ctx {
        Ctx::Server => matches!(s, "сервер" | "насервере" | "server" | "atserver"),
        Ctx::ThinClient => matches!(
            s,
            "клиент" | "наклиенте" | "тонкийклиент" | "client" | "atclient" | "thinclient"
        ),
    };
    fn or(w: &[String], i: &mut usize, on: &dyn Fn(&str) -> bool) -> bool {
        let mut v = and(w, i, on);
        while w.get(*i).is_some_and(|s| s == "или" || s == "or") {
            *i += 1;
            v |= and(w, i, on);
        }
        v
    }
    fn and(w: &[String], i: &mut usize, on: &dyn Fn(&str) -> bool) -> bool {
        let mut v = not(w, i, on);
        while w.get(*i).is_some_and(|s| s == "и" || s == "and") {
            *i += 1;
            v &= not(w, i, on);
        }
        v
    }
    fn not(w: &[String], i: &mut usize, on: &dyn Fn(&str) -> bool) -> bool {
        let Some(s) = w.get(*i) else { return false };
        *i += 1;
        match s.as_str() {
            "не" | "not" => !not(w, i, on),
            "(" => {
                let v = or(w, i, on);
                *i += 1;
                v
            }
            s => on(s),
        }
    }
    or(words, &mut 0, &on)
}

/// Убирает строки препроцессора и неактивные ветки `#Если` контекста (`None` — все ветки), а также текст между
/// `#Удаление` и `#КонецУдаления` (расширения). Проверяет парность инструкций.
fn strip_preproc(src: &str, toks: &[Token], ctx: Option<Ctx>) -> Result<Vec<Token>, Error> {
    struct Frame {
        kind: &'static str,
        parent: bool,
        taken: bool,
        active: bool,
    }
    let mut out = Vec::with_capacity(toks.len());
    let mut stack: Vec<Frame> = Vec::new();
    for t in toks {
        let active = stack.last().is_none_or(|f| f.active);
        if t.t != T::Preproc {
            if active {
                out.push(t.clone());
            }
            continue;
        }
        let text = t.text(src);
        let cond = |parent: bool| parent && ctx.is_none_or(|c| preproc_cond(text, c));
        // Закрывающая инструкция без открывающей: место — сразу за `#`.
        let missing = |m: &str| Error {
            line: t.line,
            col: t.col + 1,
            msg: format!("Пропущен оператор препроцессора {m}"),
        };
        let close = |stack: &mut Vec<Frame>, k: &str, m: &str| match stack.pop() {
            Some(f) if f.kind == k => Ok(()),
            _ => Err(missing(m)),
        };
        match preproc_word(src, t).as_str() {
            "если" | "if" => {
                let a = cond(active);
                stack.push(Frame {
                    kind: "if",
                    parent: active,
                    taken: a,
                    active: a,
                });
            }
            w @ ("иначеесли" | "elsif" | "иначе" | "else") => {
                let Some(f) = stack.last_mut().filter(|f| f.kind == "if") else {
                    return Err(missing("Если (If)"));
                };
                let a = if matches!(w, "иначе" | "else") {
                    f.parent && (ctx.is_none() || !f.taken)
                } else {
                    cond(f.parent)
                };
                f.active = a && (ctx.is_none() || !f.taken);
                f.taken |= a;
            }
            "конецесли" | "endif" => close(&mut stack, "if", "Если (If)")?,
            "область" | "region" => stack.push(Frame {
                kind: "region",
                parent: active,
                taken: true,
                active,
            }),
            "конецобласти" | "endregion" => {
                close(&mut stack, "region", "Область (Region)")?
            }
            "вставка" | "insert" => stack.push(Frame {
                kind: "insert",
                parent: active,
                taken: true,
                active,
            }),
            "конецвставки" | "endinsert" => {
                close(&mut stack, "insert", "Вставка (Insert)")?
            }
            "удаление" | "delete" => stack.push(Frame {
                kind: "delete",
                parent: active,
                taken: true,
                active: false,
            }),
            "конецудаления" | "enddelete" => {
                close(&mut stack, "delete", "Удаление (Delete)")?
            }
            _ => {
                return Err(Error {
                    line: t.line,
                    col: t.col,
                    msg: "Неизвестная инструкция препроцессора".into(),
                })
            }
        }
    }
    if let (Some(f), Some(eof)) = (stack.last(), toks.last()) {
        let m = match f.kind {
            "if" => "Если (If)",
            "region" => "Область (Region)",
            "insert" => "Вставка (Insert)",
            _ => "Удаление (Delete)",
        };
        return Err(Error {
            line: eof.line,
            col: eof.col,
            msg: format!("Ожидается завершение оператора препроцессора {m}"),
        });
    }
    Ok(out)
}

#[derive(Default)]
struct Parser<'a> {
    src: &'a str,
    t: Vec<Token>,
    i: usize,
    /// Текущий метод: функция ли (`None` — тело модуля).
    func: Option<bool>,
    asynch: bool,
    loops: u32,
    /// Вложенность блоков `Исключение`.
    excepts: u32,
    /// Можно ли еще объявлять переменные: до первого оператора метода или модуля.
    vars_ok: bool,
    labels: Vec<String>,
    gotos: Vec<(String, At)>,
    /// Имена переменных модуля и текущего метода (параметры и `Перем`) в нижнем регистре.
    mvars: std::collections::HashSet<String>,
    lvars: std::collections::HashSet<String>,
    /// Методы, которые компилируются на сервере в этом проходе (`Асинх` у них — ошибка).
    site: Site,
    server: bool,
    /// Продолжать после ошибки в методе — со следующего метода, как `/CheckModules`; ошибки — сюда.
    recover: bool,
    errors: Vec<Error>,
}

/// Где модуль компилируется на сервере — для «Недопустимое объявление асинхронной процедуры или функции»
/// (`/CheckModules -Server`, 28.10.2026: модуль объекта, серверный общий модуль, форма без директивы,
/// `&НаСервере`, `&НаКлиентеНаСервереБезКонтекста` — ошибка; `&НаКлиенте` и `#Если Клиент` — нет).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Site {
    /// Модули объекта, менеджера, набора записей, сеанса, общий модуль только «Сервер».
    Server,
    /// Модуль формы: без директивы — на сервере.
    Form,
    /// Модуль команды: на сервере — только методы с серверной директивой.
    // shortcut: у модуля команды и общего модуля «клиент и сервер» умолчание не снято — Асинх без директивы не
    // проверяется; снять пробой /CheckModules, когда понадобится.
    Command,
    /// Клиентские и неизвестные: не проверяется.
    #[default]
    Other,
}

impl Site {
    fn on_server(self, directives: &[String]) -> bool {
        let server = |d: &String| {
            let d = d.to_lowercase();
            [
                "&насервере",
                "&atserver",
                "&насерверебезконтекста",
                "&atservernocontext",
                "&наклиентенасерверебезконтекста",
                "&atclientatservernocontext",
            ]
            .contains(&d.as_str())
        };
        let client = |d: &String| ["&наклиенте", "&atclient"].contains(&d.to_lowercase().as_str());
        match self {
            Site::Server => true,
            Site::Form => directives.iter().any(server) || !directives.iter().any(client),
            Site::Command => directives.iter().any(server),
            Site::Other => false,
        }
    }
}

/// Вызовы методов модуля: процедура в выражении, число фактических параметров. Ошибка — первая по тексту.
fn check_calls(m: &Module) -> R<()> {
    calls(m)
        .into_iter()
        .min_by_key(|e| (e.line, e.col))
        .map_or(Ok(()), Err)
}

/// Все ошибки вызовов методов модуля (по `check_calls`).
fn calls(m: &Module) -> Vec<Error> {
    let methods: std::collections::HashMap<String, &Method> = m
        .methods
        .iter()
        .map(|f| (f.name.to_lowercase(), f))
        .collect();
    let mut all: Vec<Error> = Vec::new();
    let mut found = |at: At, msg: String| {
        all.push(Error {
            line: at.line,
            col: at.col,
            msg,
        })
    };
    fn expr(
        e: &Expr,
        stmt: bool,
        ms: &std::collections::HashMap<String, &Method>,
        f: &mut dyn FnMut(At, String),
    ) {
        match e {
            Expr::Call(c, args, arg_at) => {
                if let Expr::Name(n, at) = &**c {
                    if let Some(m) = ms.get(&n.to_lowercase()) {
                        if !stmt && !m.func {
                            f(*at, format!("Обращение к процедуре как к функции ({n})"));
                        }
                        let need = m
                            .params
                            .iter()
                            .rposition(|p| p.default.is_none())
                            .map_or(0, |i| i + 1);
                        if args.len() > m.params.len() {
                            // место — первый лишний аргумент
                            f(
                                arg_at[m.params.len()],
                                format!("Слишком много фактических параметров ({n})"),
                            );
                        } else if args.len() < need {
                            f(*at, format!("Недостаточно фактических параметров ({n})"));
                        }
                    }
                } else {
                    expr(c, false, ms, f);
                }
                args.iter().flatten().for_each(|a| expr(a, false, ms, f));
            }
            Expr::Member(a, ..) | Expr::Unary(_, a) | Expr::Await(a) => expr(a, false, ms, f),
            Expr::Index(a, b) | Expr::Binary(_, a, b) => {
                expr(a, false, ms, f);
                expr(b, false, ms, f);
            }
            Expr::Ternary(a, b, c) => [a, b, c].iter().for_each(|x| expr(x, false, ms, f)),
            Expr::New(_, args) => args.iter().flatten().for_each(|a| expr(a, false, ms, f)),
            _ => {}
        }
    }
    fn stmts(
        b: &[St],
        ms: &std::collections::HashMap<String, &Method>,
        f: &mut dyn FnMut(At, String),
    ) {
        for s in b {
            match &s.s {
                Stmt::Assign(a, b) | Stmt::AddHandler(a, b) | Stmt::RemoveHandler(a, b) => {
                    expr(a, false, ms, f);
                    expr(b, false, ms, f);
                }
                Stmt::Call(e) => expr(e, true, ms, f),
                Stmt::Await(e) => expr(e, true, ms, f),
                Stmt::Execute(e) | Stmt::Return(Some(e)) => expr(e, false, ms, f),
                Stmt::Raise(a) => a.iter().flatten().for_each(|e| expr(e, false, ms, f)),
                Stmt::If(arms, other) => {
                    for (c, b) in arms {
                        expr(c, false, ms, f);
                        stmts(b, ms, f);
                    }
                    other.iter().for_each(|b| stmts(b, ms, f));
                }
                Stmt::While(c, b) | Stmt::ForEach(_, _, c, b) => {
                    expr(c, false, ms, f);
                    stmts(b, ms, f);
                }
                Stmt::For(_, _, a, c, b) => {
                    expr(a, false, ms, f);
                    expr(c, false, ms, f);
                    stmts(b, ms, f);
                }
                Stmt::Try(a, b) => {
                    stmts(a, ms, f);
                    stmts(b, ms, f);
                }
                _ => {}
            }
        }
    }
    for f in &m.methods {
        f.params
            .iter()
            .filter_map(|p| p.default.as_ref())
            .for_each(|e| expr(e, false, &methods, &mut found));
        stmts(&f.body, &methods, &mut found);
    }
    stmts(&m.body, &methods, &mut found);
    all
}

const VARS_FIRST: &str =
    "Объявления переменных должны быть расположены в начале модуля, процедуры или функции";

type R<T> = Result<T, Error>;

/// Слова, которыми заканчивается последовательность операторов.
fn ends_block(k: Option<Kw>) -> bool {
    matches!(
        k,
        Some(
            Kw::EndProcedure
                | Kw::EndFunction
                | Kw::EndIf
                | Kw::ElsIf
                | Kw::Else
                | Kw::EndDo
                | Kw::Except
                | Kw::EndTry
        )
    )
}

impl<'a> Parser<'a> {
    fn tok(&self) -> &Token {
        &self.t[self.i]
    }
    fn kw(&self) -> Option<Kw> {
        self.tok().kw
    }
    fn at(&self) -> At {
        At {
            line: self.tok().line,
            col: self.tok().col,
        }
    }
    fn text(&self) -> &'a str {
        self.t[self.i].text(self.src)
    }
    fn is_op(&self, s: &str) -> bool {
        self.tok().t == T::Op && self.text() == s
    }
    fn next(&mut self) -> &Token {
        let t = &self.t[self.i];
        if t.t != T::Eof {
            self.i += 1;
        }
        t
    }
    /// Ошибка в начале текущей лексемы.
    fn err<X>(&self, msg: &str) -> R<X> {
        Err(Error {
            line: self.tok().line,
            col: self.tok().col,
            msg: msg.to_string(),
        })
    }
    /// Ошибка сразу за предыдущей лексемой — так платформа отмечает недостающее.
    fn err_prev<X>(&self, msg: &str) -> R<X> {
        match self.i.checked_sub(1).map(|i| &self.t[i]) {
            Some(p) => Err(Error {
                line: p.eline,
                col: p.ecol,
                msg: msg.to_string(),
            }),
            None => self.err(msg),
        }
    }
    fn eat_op(&mut self, s: &str) -> bool {
        let y = self.is_op(s);
        if y {
            self.next();
        }
        y
    }
    fn expect_op(&mut self, s: &str) -> R<()> {
        if self.eat_op(s) {
            Ok(())
        } else {
            self.err_prev(&format!("Ожидается символ '{s}'"))
        }
    }
    fn eat_kw(&mut self, k: Kw) -> bool {
        let y = self.kw() == Some(k);
        if y {
            self.next();
        }
        y
    }
    /// Недостающее слово конца блока платформа отмечает на лексеме, которой блок кончился, прочие — за предыдущей.
    fn expect_kw(&mut self, k: Kw) -> R<()> {
        if self.eat_kw(k) {
            return Ok(());
        }
        let m = format!("Ожидается ключевое слово {}", kw_name(k));
        if matches!(k, Kw::Then | Kw::Do | Kw::To | Kw::In) {
            self.err_prev(&m)
        } else {
            self.err(&m)
        }
    }
    /// Имя: слово, не являющееся ключевым.
    fn name(&mut self) -> R<(String, At)> {
        if self.tok().t == T::Word && self.kw().is_none() {
            let at = self.at();
            let s = self.text().to_string();
            self.next();
            Ok((s, at))
        } else {
            self.err("Ожидается идентификатор")
        }
    }

    fn var_name(&mut self) -> R<(String, At)> {
        if self.tok().t == T::Word && self.kw().is_none() {
            self.name()
        } else {
            self.err("Ожидается имя переменной")
        }
    }

    fn module(&mut self) -> R<Module> {
        let mut m = Module::default();
        let mut names = std::collections::HashSet::new();
        loop {
            let mut directives = Vec::new();
            // Незнакомая аннотация (`&НаКлентe`) в модуле обработки ошибкой у платформы не считается (oracle_bsl_errors).
            while self.tok().t == T::Directive {
                directives.push(self.text().to_string());
                self.next();
            }
            match self.kw() {
                Some(Kw::Var) => {
                    if !m.methods.is_empty() {
                        return self.err(VARS_FIRST);
                    }
                    self.next();
                    m.vars.extend(self.vars(directives, true)?);
                    self.expect_op(";")?;
                }
                Some(Kw::Procedure | Kw::Function | Kw::Async) => {
                    let f = match self.method(directives) {
                        Ok(f) => f,
                        Err(e) if self.recover => {
                            self.errors.push(e);
                            self.skip_method();
                            continue;
                        }
                        Err(e) => return Err(e),
                    };
                    if !names.insert(f.name.to_lowercase()) {
                        let msg = format!(
                            "Процедура или функция с указанным именем уже определена ({})",
                            f.name
                        );
                        let e = Error {
                            line: f.at.line,
                            col: f.at.col,
                            msg,
                        };
                        if !self.recover {
                            return Err(e);
                        }
                        self.errors.push(e);
                    }
                    m.methods.push(f);
                }
                _ if !directives.is_empty() => {
                    return self.err("Ожидается объявление процедуры, функции или переменной")
                }
                _ if self.tok().t == T::Eof => return Ok(m),
                _ => {
                    self.vars_ok = false;
                    m.body = self.stmts()?;
                    self.check_labels()?;
                    if matches!(self.kw(), Some(Kw::Procedure | Kw::Function | Kw::Async))
                        || self.tok().t == T::Directive
                    {
                        return self.err("Определения процедур и функций должны размещаться перед операторами тела модуля");
                    }
                    if self.tok().t != T::Eof {
                        return self.err("Неопознанный оператор");
                    }
                    return Ok(m);
                }
            }
        }
    }

    /// После ошибки в методе: за ближайшее `КонецПроцедуры`/`КонецФункции` (или к концу текста), состояние метода — заново.
    fn skip_method(&mut self) {
        while self.tok().t != T::Eof
            && !matches!(self.kw(), Some(Kw::EndProcedure | Kw::EndFunction))
        {
            self.next();
        }
        if self.tok().t != T::Eof {
            self.next();
        }
        (
            self.func,
            self.asynch,
            self.loops,
            self.excepts,
            self.vars_ok,
        ) = (None, false, 0, 0, true);
        self.labels.clear();
        self.gotos.clear();
    }

    /// Переходы на метки, которых нет в методе (теле модуля); место — конец метода.
    fn check_labels(&mut self) -> R<()> {
        let gotos = std::mem::take(&mut self.gotos);
        let labels = std::mem::take(&mut self.labels);
        match gotos.iter().find(|g| {
            !labels
                .iter()
                .any(|l| l.to_lowercase() == g.0.to_lowercase())
        }) {
            Some((g, _)) => self.err(&format!(
                "Метка не определена в процедуре, функции или теле модуля (~{g})"
            )),
            None => Ok(()),
        }
    }

    fn vars(&mut self, directives: Vec<String>, module: bool) -> R<Vec<Var>> {
        let mut out = Vec::new();
        loop {
            let (name, at) = self.var_name()?;
            let set = if module {
                &mut self.mvars
            } else {
                &mut self.lvars
            };
            if !set.insert(name.to_lowercase()) {
                return Err(Error {
                    line: at.line,
                    col: at.col,
                    msg: format!("Переменная с указанным именем уже определена ({name})"),
                });
            }
            if !module && self.kw() == Some(Kw::Export) {
                return self.err(&format!(
                    "Локальная переменная не может быть экспортирована ({name})"
                ));
            }
            let export = module && self.eat_kw(Kw::Export);
            out.push(Var {
                name,
                at,
                export,
                directives: directives.clone(),
            });
            if !self.eat_op(",") {
                return Ok(out);
            }
        }
    }

    fn method(&mut self, directives: Vec<String>) -> R<Method> {
        let asynch = self.kw() == Some(Kw::Async);
        if asynch && self.server && self.site.on_server(&directives) {
            return self.err("Недопустимое объявление асинхронной процедуры или функции");
        }
        self.eat_kw(Kw::Async);
        let func = match self.kw() {
            Some(Kw::Procedure) => false,
            Some(Kw::Function) => true,
            _ => return self.err("Ожидается ключевое слово 'Процедура' или 'Функция'"),
        };
        self.next();
        let (name, at) = self.name()?;
        self.expect_op("(")?;
        let mut params = Vec::new();
        self.lvars.clear();
        if !self.eat_op(")") {
            loop {
                let val = self.eat_kw(Kw::Val);
                let (name, at) = self.name()?;
                if !self.lvars.insert(name.to_lowercase()) {
                    return Err(Error {
                        line: at.line,
                        col: at.col,
                        msg: format!(
                            "Формальный параметр с указанным именем уже определен ({name})"
                        ),
                    });
                }
                let default = if self.eat_op("=") {
                    Some(self.unary()?)
                } else {
                    None
                };
                params.push(Param {
                    name,
                    at,
                    val,
                    default,
                });
                if self.eat_op(")") {
                    break;
                }
                self.expect_op(",")?;
            }
        }
        let export = self.eat_kw(Kw::Export);
        (self.func, self.asynch, self.loops, self.vars_ok) = (Some(func), asynch, 0, true);
        let body = self.stmts()?;
        self.check_labels()?;
        (self.func, self.asynch) = (None, false);
        let end_line = self.tok().line;
        if func {
            self.expect_kw(Kw::EndFunction)?;
        } else {
            self.expect_kw(Kw::EndProcedure)?;
        }
        Ok(Method {
            name,
            at,
            func,
            asynch,
            export,
            directives,
            params,
            body,
            end_line,
        })
    }

    /// Операторы через `;` до слова конца блока или конца текста.
    fn stmts(&mut self) -> R<Vec<St>> {
        let mut out = Vec::new();
        loop {
            while self.eat_op(";") {}
            if self.tok().t == T::Eof || ends_block(self.kw()) {
                return Ok(out);
            }
            if matches!(self.kw(), Some(Kw::Procedure | Kw::Function | Kw::Async))
                || self.tok().t == T::Directive
            {
                return Ok(out);
            }
            let line = self.tok().line;
            if self.tok().t == T::Label {
                let l = self.text()[1..].to_string();
                self.next();
                self.expect_op(":")?;
                if self
                    .labels
                    .iter()
                    .any(|x| x.to_lowercase() == l.to_lowercase())
                {
                    return Err(Error {
                        line,
                        col: self.t[self.i - 2].col,
                        msg: "Метка с указанным именем уже определена".into(),
                    });
                }
                self.labels.push(l.clone());
                out.push(St {
                    line,
                    s: Stmt::Label(l),
                });
                continue;
            }
            if self.kw() == Some(Kw::Var) && !self.vars_ok {
                return self.err(VARS_FIRST);
            }
            let s = self.stmt()?;
            if !matches!(s, Stmt::Var(_)) {
                self.vars_ok = false;
            }
            out.push(St { line, s });
            if self.eat_op(";") || self.tok().t == T::Eof || ends_block(self.kw()) {
                continue;
            }
            // Следующая лексема на новой строке — забыта `;`; на той же — лишнее в операторе.
            let prev = &self.t[self.i - 1];
            return if self.tok().line > prev.eline {
                self.err_prev("Ожидается символ ';'")
            } else {
                self.err("Неопознанный оператор")
            };
        }
    }

    /// Вложенный блок: переменные в нем не объявляются.
    fn block(&mut self) -> R<Vec<St>> {
        self.vars_ok = false;
        self.stmts()
    }

    fn block_until(&mut self, end: Kw) -> R<Vec<St>> {
        let b = self.block()?;
        self.expect_kw(end)?;
        Ok(b)
    }

    fn loop_until(&mut self, end: Kw) -> R<Vec<St>> {
        self.loops += 1;
        let b = self.block_until(end);
        self.loops -= 1;
        b
    }

    fn stmt(&mut self) -> R<Stmt> {
        let Some(k) = self.kw() else {
            let e = self.postfix()?;
            if self.eat_op("=") {
                return Ok(Stmt::Assign(e, self.expr()?));
            }
            return if matches!(e, Expr::Call(..)) {
                Ok(Stmt::Call(e))
            } else {
                self.err("Неопознанный оператор")
            };
        };
        if k == Kw::Await && !self.asynch {
            return self.err("Оператор Ждать (Await) может употребляться только в асинхронных процедурах или функциях");
        }
        self.next();
        Ok(match k {
            Kw::If => {
                let mut arms = Vec::new();
                let mut other = None;
                loop {
                    let c = self.expr()?;
                    self.expect_kw(Kw::Then)?;
                    arms.push((c, self.block()?));
                    if self.eat_kw(Kw::ElsIf) {
                        continue;
                    }
                    if self.eat_kw(Kw::Else) {
                        other = Some(self.block()?);
                    }
                    self.expect_kw(Kw::EndIf)?;
                    break;
                }
                Stmt::If(arms, other)
            }
            Kw::While => {
                let c = self.expr()?;
                self.expect_kw(Kw::Do)?;
                Stmt::While(c, self.loop_until(Kw::EndDo)?)
            }
            Kw::For => {
                if self.eat_kw(Kw::Each) {
                    let (n, at) = self.var_name()?;
                    self.expect_kw(Kw::In)?;
                    let c = self.expr()?;
                    self.expect_kw(Kw::Do)?;
                    Stmt::ForEach(n, at, c, self.loop_until(Kw::EndDo)?)
                } else {
                    let (n, at) = self.var_name()?;
                    self.expect_op("=")?;
                    let a = self.expr()?;
                    self.expect_kw(Kw::To)?;
                    let b = self.expr()?;
                    self.expect_kw(Kw::Do)?;
                    Stmt::For(n, at, a, b, self.loop_until(Kw::EndDo)?)
                }
            }
            Kw::Try => {
                let a = self.block_until(Kw::Except)?;
                self.excepts += 1;
                let b = self.block_until(Kw::EndTry);
                self.excepts -= 1;
                Stmt::Try(a, b?)
            }
            Kw::Raise if self.stmt_end() && self.excepts == 0 => {
                return self.err_prev(
                    "Оператор ВызватьИсключение (Raise) без аргументов может употребляться только при обработке исключения",
                );
            }
            Kw::Return => {
                if self.func.is_none() {
                    return self.err_prev("Оператор Возврат (Return) не может употребляться вне процедуры или функции");
                }
                if self.stmt_end() {
                    if self.func == Some(true) {
                        return self.err_prev("Ожидается выражение");
                    }
                    Stmt::Return(None)
                } else {
                    let e = self.expr()?;
                    if self.func == Some(false) {
                        return self.err_prev("Процедура не может возвращать значение");
                    }
                    Stmt::Return(Some(e))
                }
            }
            Kw::Continue | Kw::Break if self.loops == 0 => {
                let w = if k == Kw::Continue {
                    "Продолжить (Continue)"
                } else {
                    "Прервать (Break)"
                };
                return self.err_prev(&format!(
                    "Оператор {w} может употребляться только внутри цикла"
                ));
            }
            Kw::Continue => Stmt::Continue,
            Kw::Break => Stmt::Break,
            Kw::Raise => Stmt::Raise(if self.stmt_end() {
                Vec::new()
            } else if self.is_op("(") {
                self.args()?
            } else {
                vec![Some(self.expr()?)]
            }),
            Kw::Execute => Stmt::Execute(self.expr()?),
            Kw::Goto => {
                if self.tok().t != T::Label {
                    return self.err("Ожидается метка");
                }
                let l = self.text()[1..].to_string();
                self.gotos.push((l.clone(), self.at()));
                self.next();
                Stmt::Goto(l)
            }
            Kw::AddHandler | Kw::RemoveHandler => {
                let a = self.postfix()?;
                self.expect_op(",")?;
                let b = self.postfix()?;
                if k == Kw::AddHandler {
                    Stmt::AddHandler(a, b)
                } else {
                    Stmt::RemoveHandler(a, b)
                }
            }
            Kw::Var => Stmt::Var(self.vars(Vec::new(), false)?),
            Kw::Await => Stmt::Await(self.unary()?),
            _ => {
                self.i -= 1;
                return self.err("Ожидается оператор");
            }
        })
    }

    fn stmt_end(&self) -> bool {
        self.is_op(";") || self.tok().t == T::Eof || ends_block(self.kw())
    }

    fn expr(&mut self) -> R<Expr> {
        let mut a = self.and()?;
        while self.eat_kw(Kw::Or) {
            a = Expr::Binary("ИЛИ", Box::new(a), Box::new(self.and()?));
        }
        Ok(a)
    }
    fn and(&mut self) -> R<Expr> {
        let mut a = self.not()?;
        while self.eat_kw(Kw::And) {
            a = Expr::Binary("И", Box::new(a), Box::new(self.not()?));
        }
        Ok(a)
    }
    fn not(&mut self) -> R<Expr> {
        if self.eat_kw(Kw::Not) {
            return Ok(Expr::Unary("НЕ", Box::new(self.not()?)));
        }
        self.cmp()
    }
    fn cmp(&mut self) -> R<Expr> {
        let mut a = self.add()?;
        loop {
            let op = match self.text() {
                "=" => "=",
                "<>" => "<>",
                "<" => "<",
                ">" => ">",
                "<=" => "<=",
                ">=" => ">=",
                _ => return Ok(a),
            };
            if self.tok().t != T::Op {
                return Ok(a);
            }
            self.next();
            a = Expr::Binary(op, Box::new(a), Box::new(self.operand(Self::add)?));
        }
    }
    fn add(&mut self) -> R<Expr> {
        let mut a = self.mul()?;
        loop {
            let op = if self.is_op("+") {
                "+"
            } else if self.is_op("-") {
                "-"
            } else {
                return Ok(a);
            };
            self.next();
            a = Expr::Binary(op, Box::new(a), Box::new(self.operand(Self::mul)?));
        }
    }
    fn mul(&mut self) -> R<Expr> {
        let mut a = self.unary()?;
        loop {
            let op = match self.text() {
                "*" => "*",
                "/" => "/",
                "%" => "%",
                _ => return Ok(a),
            };
            if self.tok().t != T::Op {
                return Ok(a);
            }
            self.next();
            a = Expr::Binary(op, Box::new(a), Box::new(self.operand(Self::unary)?));
        }
    }
    /// Правый операнд знака операции: нет его сразу за знаком — «Ошибка в выражении», как у платформы (`1 +;`,
    /// oracle_bsl_undefined), а не «Ожидается выражение».
    fn operand(&mut self, f: fn(&mut Self) -> R<Expr>) -> R<Expr> {
        let op = &self.t[self.i - 1];
        let (line, col) = (op.eline, op.ecol);
        f(self).map_err(|e| {
            if e.line == line && e.col == col && e.msg == "Ожидается выражение" {
                Error {
                    msg: "Ошибка в выражении".into(),
                    ..e
                }
            } else {
                e
            }
        })
    }

    fn unary(&mut self) -> R<Expr> {
        if self.eat_op("-") {
            return Ok(Expr::Unary("-", Box::new(self.unary()?)));
        }
        if self.eat_op("+") {
            return Ok(Expr::Unary("+", Box::new(self.unary()?)));
        }
        if self.kw() == Some(Kw::Await) {
            if !self.asynch {
                return self.err("Оператор Ждать (Await) может употребляться только в асинхронных процедурах или функциях");
            }
            self.next();
            return Ok(Expr::Await(Box::new(self.unary()?)));
        }
        self.postfix()
    }

    /// Аргументы вызова в скобках; пропущенный аргумент — `None`.
    fn args(&mut self) -> R<Vec<Option<Expr>>> {
        Ok(self.args_at()?.0)
    }

    fn args_at(&mut self) -> R<(Vec<Option<Expr>>, Vec<At>)> {
        self.expect_op("(")?;
        let (mut out, mut at) = (Vec::new(), Vec::new());
        if self.eat_op(")") {
            return Ok((out, at));
        }
        loop {
            at.push(self.at());
            out.push(if self.is_op(",") || self.is_op(")") {
                None
            } else {
                Some(self.expr()?)
            });
            if self.eat_op(")") {
                return Ok((out, at));
            }
            self.expect_op(",")?;
        }
    }

    fn postfix(&mut self) -> R<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.eat_op(".") {
                // После точки допустимо любое слово, в том числе ключевое (`Запрос.Выполнить()`).
                if self.tok().t != T::Word {
                    return self.err("Ожидается идентификатор");
                }
                let at = self.at();
                let n = self.text().to_string();
                self.next();
                e = Expr::Member(Box::new(e), n, at);
            } else if self.eat_op("[") {
                let i = self.expr()?;
                self.expect_op("]")?;
                e = Expr::Index(Box::new(e), Box::new(i));
            } else if self.is_op("(") {
                if !matches!(e, Expr::Name(..) | Expr::Member(..)) {
                    return self.err("Ожидается символ ';'");
                }
                let (a, at) = self.args_at()?;
                e = Expr::Call(Box::new(e), a, at);
            } else {
                return Ok(e);
            }
        }
    }

    fn primary(&mut self) -> R<Expr> {
        let t = self.tok().clone();
        let text = t.text(self.src);
        match t.t {
            T::Num => {
                self.next();
                Ok(Expr::Num(text.to_string()))
            }
            T::Str => {
                let mut s = String::new();
                while self.tok().t == T::Str {
                    s.push_str(self.text());
                    self.next();
                }
                Ok(Expr::Str(s))
            }
            T::Date => {
                self.next();
                Ok(Expr::Date(text.to_string()))
            }
            T::Op if text == "(" => {
                self.next();
                let e = self.expr()?;
                self.expect_op(")")?;
                Ok(e)
            }
            T::Op if text == "?" => {
                self.next();
                self.expect_op("(")?;
                let c = self.expr()?;
                self.expect_op(",")?;
                let a = self.expr()?;
                self.expect_op(",")?;
                let b = self.expr()?;
                self.expect_op(")")?;
                Ok(Expr::Ternary(Box::new(c), Box::new(a), Box::new(b)))
            }
            T::Word => match t.kw {
                None => {
                    self.next();
                    Ok(Expr::Name(
                        text.to_string(),
                        At {
                            line: t.line,
                            col: t.col,
                        },
                    ))
                }
                Some(Kw::True | Kw::False) => {
                    self.next();
                    Ok(Expr::Bool(t.kw == Some(Kw::True)))
                }
                Some(Kw::Undefined) => {
                    self.next();
                    Ok(Expr::Undefined)
                }
                Some(Kw::Null) => {
                    self.next();
                    Ok(Expr::Null)
                }
                Some(Kw::New) => {
                    self.next();
                    if self.is_op("(") {
                        return Ok(Expr::New(None, self.args()?));
                    }
                    let (n, _) = self.name()?;
                    let a = if self.is_op("(") {
                        self.args()?
                    } else {
                        Vec::new()
                    };
                    Ok(Expr::New(Some(n), a))
                }
                _ => self.err_prev("Ожидается выражение"),
            },
            _ => self.err_prev("Ожидается выражение"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Места — как у `/CheckModules -Server -ThinClient` (28.10.2026, common_metrics).
    /// Проба /CheckModules 28.10.2026: две ошибки синтаксиса в разных методах — обе.
    #[test]
    fn all_errors() {
        let src = "Процедура А() Экспорт
	Б = 1 +;
КонецПроцедуры
Процедура В()
	Г = ;
КонецПроцедуры
";
        let e: Vec<(u32, u32)> = check_all(src, Site::Other, None)
            .into_iter()
            .map(|e| (e.line, e.col))
            .collect();
        assert_eq!(e, [(2, 9), (5, 5)]);
        assert_eq!(parse(src).unwrap_err().line, 2, "первая — как раньше");
    }

    #[test]
    fn async_on_server() {
        let a = "Асинх Процедура А() Экспорт\n\tЖдать Ф();\nКонецПроцедуры\nФункция Ф()\n\tВозврат 1;\nКонецФункции\n";
        let msg = "Недопустимое объявление асинхронной процедуры или функции";
        let at = |src: &str, site| parse_in(src, site).err().map(|e| (e.line, e.col, e.msg));
        assert_eq!(at(a, Site::Server), Some((1, 1, msg.into())));
        assert_eq!(
            at(&format!("Перем Х;\n\n{a}"), Site::Server),
            Some((3, 1, msg.into()))
        );
        assert_eq!(
            at(
                &format!("#Если Клиент Тогда\n{a}#КонецЕсли\n"),
                Site::Server
            ),
            None
        );
        assert_eq!(
            at(&format!("&НаСервере\n{a}"), Site::Form),
            Some((2, 1, msg.into()))
        );
        assert_eq!(at(a, Site::Form), Some((1, 1, msg.into())));
        assert_eq!(
            at(&format!("&НаКлиентеНаСервереБезКонтекста\n{a}"), Site::Form),
            Some((2, 1, msg.into()))
        );
        assert_eq!(at(&format!("&НаКлиенте\n{a}"), Site::Form), None);
        assert_eq!(at(a, Site::Other), None);
    }

    #[test]
    fn module_shape() {
        let m = parse(
            "&НаКлиенте\nПерем А Экспорт, Б;\n\n#Область Методы\n&НаСервере\nФункция Ф(Знач П1, П2 = -1) Экспорт\n\
             \tЗапрос = Новый Запрос(\"ВЫБРАТЬ\n\t|\t1\"); // текст\n\tВозврат ?(П1 > 0, Запрос.Выполнить().Выбрать(), Неопределено)\nКонецФункции\n#КонецОбласти\n\
             Для Каждого Х Из М Цикл Продолжить КонецЦикла;\nА = Б[1].В(, 2) + '20200101';\n",
        )
        .unwrap();
        assert_eq!(m.vars.len(), 2);
        assert!(m.vars[0].export && !m.vars[1].export);
        let f = &m.methods[0];
        assert_eq!(
            (
                f.name.as_str(),
                f.func,
                f.export,
                f.params.len(),
                f.end_line
            ),
            ("Ф", true, true, 2, 10)
        );
        assert_eq!(f.directives, ["&НаСервере"]);
        assert!(f.params[0].val && f.params[1].default.is_some());
        assert_eq!(m.body.len(), 2);
    }

    #[test]
    fn errors() {
        // Места и тексты — как у /CheckModules (tests/oracle.rs, oracle_bsl_errors).
        let e = |s: &str| {
            let e = parse(s).unwrap_err();
            (e.line, e.col, e.msg)
        };
        let m = |l, c, s: &str| (l, c, s.to_string());
        assert_eq!(e("А = 1\nБ = 2;"), m(1, 6, "Ожидается символ ';'"));
        assert_eq!(e("А = 1 2;"), m(1, 7, "Неопознанный оператор"));
        assert_eq!(
            e("Процедура П()\nЕсли А Тогда\nКонецПроцедуры"),
            m(3, 1, "Ожидается ключевое слово 'КонецЕсли' ('EndIf')")
        );
        assert_eq!(
            e("#Область А\n"),
            m(
                2,
                1,
                "Ожидается завершение оператора препроцессора Область (Region)"
            )
        );
        assert_eq!(
            e("А = \"строка\nБ = 1;"),
            m(1, 12, "Пропущен символ '\"' (двойная кавычка)")
        );
        assert_eq!(e("А;"), m(1, 2, "Неопознанный оператор"));
        assert_eq!(
            e("Процедура П()\nВозврат 1;\nКонецПроцедуры"),
            m(2, 10, "Процедура не может возвращать значение")
        );
        assert_eq!(e("Процедура П()\nПрервать;\nКонецПроцедуры").0, 2);
        assert_eq!(e("Процедура П()\nПерейти ~М;\nКонецПроцедуры").0, 3);
        assert_eq!(
            e("Процедура П()\nКонецПроцедуры\nПроцедура п()\nКонецПроцедуры").1,
            11
        );
        assert_eq!(e("А = '2020';").2, "Неправильная константа типа Дата");
    }
}
