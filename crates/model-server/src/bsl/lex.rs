//! Лексемы встроенного языка: слова, числа, строки (с продолжением `|`), даты, знаки, препроцессор `#`,
//! директивы и аннотации `&`, метки `~`.
//!
//! Происхождение: перенесено без изменений логики из constructor `src/bsl/lex.rs` (собственный код проекта).

use super::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kw {
    Procedure,
    EndProcedure,
    Function,
    EndFunction,
    If,
    Then,
    ElsIf,
    Else,
    EndIf,
    For,
    Each,
    In,
    To,
    Do,
    EndDo,
    While,
    Return,
    Continue,
    Break,
    Var,
    Export,
    Val,
    Try,
    Except,
    EndTry,
    Raise,
    New,
    And,
    Or,
    Not,
    True,
    False,
    Undefined,
    Null,
    Execute,
    Async,
    Await,
    Goto,
    AddHandler,
    RemoveHandler,
}

const KW: &[(&str, &str, Kw)] = &[
    ("процедура", "procedure", Kw::Procedure),
    ("конецпроцедуры", "endprocedure", Kw::EndProcedure),
    ("функция", "function", Kw::Function),
    ("конецфункции", "endfunction", Kw::EndFunction),
    ("если", "if", Kw::If),
    ("тогда", "then", Kw::Then),
    ("иначеесли", "elsif", Kw::ElsIf),
    ("иначе", "else", Kw::Else),
    ("конецесли", "endif", Kw::EndIf),
    ("для", "for", Kw::For),
    ("каждого", "each", Kw::Each),
    ("из", "in", Kw::In),
    ("по", "to", Kw::To),
    ("цикл", "do", Kw::Do),
    ("конеццикла", "enddo", Kw::EndDo),
    ("пока", "while", Kw::While),
    ("возврат", "return", Kw::Return),
    ("продолжить", "continue", Kw::Continue),
    ("прервать", "break", Kw::Break),
    ("перем", "var", Kw::Var),
    ("экспорт", "export", Kw::Export),
    ("знач", "val", Kw::Val),
    ("попытка", "try", Kw::Try),
    ("исключение", "except", Kw::Except),
    ("конецпопытки", "endtry", Kw::EndTry),
    ("вызватьисключение", "raise", Kw::Raise),
    ("новый", "new", Kw::New),
    ("и", "and", Kw::And),
    ("или", "or", Kw::Or),
    ("не", "not", Kw::Not),
    ("истина", "true", Kw::True),
    ("ложь", "false", Kw::False),
    ("неопределено", "undefined", Kw::Undefined),
    ("null", "null", Kw::Null),
    ("выполнить", "execute", Kw::Execute),
    ("асинх", "async", Kw::Async),
    ("ждать", "await", Kw::Await),
    ("перейти", "goto", Kw::Goto),
    ("добавитьобработчик", "addhandler", Kw::AddHandler),
    ("удалитьобработчик", "removehandler", Kw::RemoveHandler),
];

pub fn keyword(lower: &str) -> Option<Kw> {
    KW.iter()
        .find(|k| k.0 == lower || k.1 == lower)
        .map(|k| k.2)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum T {
    Word,
    Num,
    Str,
    Date,
    /// Знак: `+ - * / % = <> < > <= >= ( ) [ ] , ; . ? :`.
    Op,
    /// Строка препроцессора `#…` до конца строки.
    Preproc,
    /// Директива или аннотация: `&Имя` или `&Имя(…)`.
    Directive,
    /// Метка `~Имя`.
    Label,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub t: T,
    /// Байтовые смещения в тексте.
    pub start: usize,
    pub end: usize,
    /// Строка и колонка (в знаках) от 1.
    pub line: u32,
    pub col: u32,
    /// Место сразу за лексемой.
    pub eline: u32,
    pub ecol: u32,
    pub kw: Option<Kw>,
}

impl Token {
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start..self.end]
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

struct Lexer<'a> {
    s: &'a str,
    i: usize,
    line: u32,
    col: u32,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.s[self.i..].chars().next()
    }
    fn peek2(&self) -> Option<char> {
        self.s[self.i..].chars().nth(1)
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }
    fn skip_line(&mut self) {
        while self.peek().is_some_and(|c| c != '\n') {
            self.bump();
        }
    }
    fn err(&self, line: u32, col: u32, msg: &str) -> Error {
        Error {
            line,
            col,
            msg: msg.to_string(),
        }
    }
    /// Тело строки после открывающей кавычки; на конце строки текста — продолжение с `|` (между ними — пустые
    /// строки и комментарии).
    fn string(&mut self) -> Result<(), Error> {
        loop {
            let rest = &self.s[self.i..];
            if rest.is_empty() || rest.starts_with('\n') || rest.starts_with("\r\n") {
                // Место ошибки — конец строки текста, на которой строка оборвалась.
                let e = self.err(
                    self.line,
                    self.col,
                    "Пропущен символ '\"' (двойная кавычка)",
                );
                loop {
                    while self.peek().is_some_and(|c| c.is_whitespace()) {
                        self.bump();
                    }
                    if self.s[self.i..].starts_with("//") {
                        self.skip_line();
                        continue;
                    }
                    if self.peek() == Some('|') {
                        self.bump();
                        break;
                    }
                    return Err(e);
                }
                continue;
            }
            if self.bump() == Some('"') {
                if self.peek() == Some('"') {
                    self.bump();
                } else {
                    return Ok(());
                }
            }
        }
    }
}

pub fn tokens(src: &str) -> Result<Vec<Token>, Error> {
    let mut l = Lexer {
        s: src,
        i: 0,
        line: 1,
        col: 1,
    };
    let mut out = Vec::new();
    loop {
        while l
            .peek()
            .is_some_and(|c| c.is_whitespace() || c == '\u{feff}')
        {
            l.bump();
        }
        let (start, line, col) = (l.i, l.line, l.col);
        let Some(c) = l.peek() else {
            out.push(Token {
                t: T::Eof,
                start,
                end: start,
                line,
                col,
                eline: line,
                ecol: col,
                kw: None,
            });
            return Ok(out);
        };
        let t = if c == '/' && l.peek2() == Some('/') {
            l.skip_line();
            continue;
        } else if c == '#' {
            l.skip_line();
            T::Preproc
        } else if c == '"' {
            l.bump();
            l.string()?;
            T::Str
        } else if c == '\'' {
            l.bump();
            while l.peek().is_some_and(|c| c != '\'' && c != '\n') {
                l.bump();
            }
            let digits = src[start..l.i].chars().filter(char::is_ascii_digit).count();
            // shortcut: проверяется только число цифр (ГГГГММДД[ччмм[сс]]), не сама дата — до первого расхождения с платформой.
            // Незакрытую дату платформа тоже считает неправильной константой, в ее начале (oracle_bsl_errors).
            if l.bump() != Some('\'') || ![8, 12, 14].contains(&digits) {
                return Err(l.err(line, col, "Неправильная константа типа Дата"));
            }
            T::Date
        } else if c.is_ascii_digit() {
            while l.peek().is_some_and(|c| c.is_ascii_digit()) {
                l.bump();
            }
            if l.peek() == Some('.') && l.peek2().is_some_and(|c| c.is_ascii_digit()) {
                l.bump();
                while l.peek().is_some_and(|c| c.is_ascii_digit()) {
                    l.bump();
                }
            }
            T::Num
        } else if is_word(c) {
            while l.peek().is_some_and(is_word) {
                l.bump();
            }
            T::Word
        } else if c == '&' || c == '~' {
            l.bump();
            while l.peek().is_some_and(is_word) {
                l.bump();
            }
            if c == '~' {
                T::Label
            } else {
                // Параметры аннотации: `&Перед("Имя")`.
                let save = (l.i, l.line, l.col);
                while l.peek().is_some_and(|c| c == ' ' || c == '\t') {
                    l.bump();
                }
                if l.peek() == Some('(') {
                    while let Some(c) = l.bump() {
                        if c == '"' {
                            l.string()?;
                        } else if c == ')' || c == '\n' {
                            break;
                        }
                    }
                } else {
                    (l.i, l.line, l.col) = save;
                }
                T::Directive
            }
        } else {
            l.bump();
            if matches!((c, l.peek()), ('<', Some('>' | '=')) | ('>', Some('='))) {
                l.bump();
            } else if !"+-*/%=<>()[],;.?:".contains(c) {
                // Платформа читает текст до недопустимого знака: дальше для нее конец модуля.
                out.push(Token {
                    t: T::Eof,
                    start,
                    end: start,
                    line,
                    col,
                    eline: line,
                    ecol: col,
                    kw: None,
                });
                return Ok(out);
            }
            T::Op
        };
        let kw = if t == T::Word {
            keyword(&src[start..l.i].to_lowercase())
        } else {
            None
        };
        out.push(Token {
            t,
            start,
            end: l.i,
            line,
            col,
            eline: l.line,
            ecol: l.col,
            kw,
        });
    }
}
