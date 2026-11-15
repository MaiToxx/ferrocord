//! Coloration syntaxique simple des blocs de code : mots-clés, chaînes,
//! nombres, commentaires, pour quelques langages courants. Un seul passage,
//! linéaire ; un langage inconnu (ou absent) n'est pas coloré.

use super::{Span, SpanKind, Style};

/// Classe d'un morceau de code (sa couleur vient du thème).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Plain,
    Keyword,
    String,
    Number,
    Comment,
    /// Ligne ajoutée d'un `diff`.
    Added,
    /// Ligne retirée d'un `diff`.
    Removed,
}

struct Lang {
    keywords: &'static [&'static str],
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    quotes: &'static [u8],
    /// Mots-clés sans tenir compte de la casse (SQL).
    any_case: bool,
}

const RUST: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "Some", "None", "Ok", "Err",
];
const C_LIKE: &[&str] = &[
    "auto", "bool", "break", "case", "catch", "char", "class", "const", "continue", "default", "delete", "do", "double", "else", "enum", "extends",
    "extern", "false", "final", "finally", "float", "for", "func", "go", "goto", "if", "implements", "import", "include", "int", "interface", "long",
    "namespace", "new", "null", "nullptr", "package", "private", "protected", "public", "return", "short", "signed", "sizeof", "static", "struct", "switch",
    "template", "this", "throw", "throws", "true", "try", "typedef", "typename", "union", "unsigned", "using", "var", "virtual", "void", "volatile",
    "while", "string", "override", "readonly", "fun", "val", "when", "is", "defer", "chan", "map", "range", "type", "nil",
];
const JS: &[&str] = &[
    "async", "await", "break", "case", "catch", "class", "const", "continue", "default", "delete", "do", "else", "export", "extends", "false", "finally",
    "for", "from", "function", "if", "import", "in", "instanceof", "interface", "let", "new", "null", "of", "return", "static", "super", "switch", "this",
    "throw", "true", "try", "type", "typeof", "undefined", "var", "void", "while", "yield", "enum", "implements", "private", "public", "readonly", "as",
];
const PY: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "False", "finally", "for", "from",
    "global", "if", "import", "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "self", "True", "try", "while", "with",
    "yield",
];
const SH: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "in", "function", "return", "export", "local", "echo", "cd", "set",
    "param", "foreach", "true", "false",
];
const SQL: &[&str] = &[
    "select", "from", "where", "insert", "into", "values", "update", "set", "delete", "create", "table", "drop", "alter", "and", "or", "not", "null",
    "join", "left", "right", "inner", "outer", "on", "group", "by", "order", "limit", "as", "distinct", "having", "union", "primary", "key", "is", "in",
];
const LUA: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true",
    "until", "while",
];
const JSON: &[&str] = &["true", "false", "null"];

fn lang(name: &str) -> Option<Lang> {
    let c_comments = (&["//"][..], Some(("/*", "*/")));
    let l = |keywords, (line_comments, block_comment): (&'static [&'static str], Option<(&'static str, &'static str)>), quotes| Lang {
        keywords,
        line_comments,
        block_comment,
        quotes,
        any_case: false,
    };
    Some(match name.to_ascii_lowercase().as_str() {
        "rs" | "rust" => l(RUST, c_comments, b"\""),
        "c" | "h" | "cpp" | "c++" | "hpp" | "cc" | "cs" | "csharp" | "java" | "kt" | "kotlin" | "go" | "golang" | "swift" | "dart" => l(C_LIKE, c_comments, b"\"'"),
        "js" | "javascript" | "jsx" | "ts" | "typescript" | "tsx" | "mjs" => l(JS, c_comments, b"\"'`"),
        "json" | "jsonc" => l(JSON, c_comments, b"\""),
        "py" | "python" => l(PY, (&["#"], None), b"\"'"),
        "sh" | "bash" | "zsh" | "shell" | "ps1" | "powershell" | "ps" => l(SH, (&["#"], None), b"\"'"),
        "toml" | "ini" | "yaml" | "yml" | "conf" => l(JSON, (&["#", ";"], None), b"\"'"),
        "lua" => l(LUA, (&["--"], None), b"\"'"),
        "sql" => Lang { any_case: true, ..l(SQL, (&["--"], Some(("/*", "*/"))), b"'\"") },
        "css" | "scss" => l(&[], (&[], Some(("/*", "*/"))), b"\"'"),
        _ => return None,
    })
}

/// Morceaux colorés d'un bloc de code (langage `lang`, s'il est connu).
pub fn highlight(lang_name: Option<&str>, code: &str) -> Vec<Span> {
    let mut out = Vec::new();
    match lang_name.map(str::to_ascii_lowercase).as_deref() {
        Some("diff") => diff(code, &mut out),
        Some(name) => match lang(name) {
            Some(l) => tokens(&l, code, &mut out),
            None => push(&mut out, code, Class::Plain),
        },
        None => push(&mut out, code, Class::Plain),
    }
    out
}

fn push(out: &mut Vec<Span>, text: &str, class: Class) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut()
        && last.kind == SpanKind::Code(class)
    {
        last.text.push_str(text);
        return;
    }
    out.push(Span { text: text.to_string(), style: Style::default(), kind: SpanKind::Code(class) });
}

fn diff(code: &str, out: &mut Vec<Span>) {
    for line in code.split_inclusive('\n') {
        let class = match line.as_bytes().first() {
            Some(b'+') => Class::Added,
            Some(b'-') => Class::Removed,
            Some(b'@') => Class::Keyword,
            _ => Class::Plain,
        };
        push(out, line, class);
    }
}

fn tokens(l: &Lang, code: &str, out: &mut Vec<Span>) {
    let b = code.as_bytes();
    let mut i = 0;
    let mut plain = 0;
    let flush = |out: &mut Vec<Span>, from: usize, to: usize| push(out, &code[from..to], Class::Plain);
    while i < b.len() {
        let rest = &code[i..];
        // Commentaire de fin de ligne.
        if l.line_comments.iter().any(|c| rest.starts_with(c)) {
            flush(out, plain, i);
            let end = rest.find('\n').map_or(b.len(), |e| i + e);
            push(out, &code[i..end], Class::Comment);
            i = end;
            plain = i;
            continue;
        }
        if let Some((open, close)) = l.block_comment
            && rest.starts_with(open)
        {
            flush(out, plain, i);
            let end = rest[open.len()..].find(close).map_or(b.len(), |e| i + open.len() + e + close.len());
            push(out, &code[i..end], Class::Comment);
            i = end;
            plain = i;
            continue;
        }
        let c = b[i];
        if l.quotes.contains(&c) {
            flush(out, plain, i);
            let mut j = i + 1;
            while j < b.len() {
                match b[j] {
                    b'\\' => j += 2,
                    x if x == c => {
                        j += 1;
                        break;
                    }
                    // Une chaîne non fermée s'arrête en fin de ligne (sauf les gabarits `…`).
                    b'\n' if c != b'`' => break,
                    _ => j += 1,
                }
            }
            let j = j.min(b.len());
            push(out, &code[i..j], Class::String);
            i = j;
            plain = i;
            continue;
        }
        if c.is_ascii_digit() && (i == 0 || !is_ident(b[i - 1])) {
            flush(out, plain, i);
            let j = b[i..].iter().position(|x| !(x.is_ascii_alphanumeric() || *x == b'.' || *x == b'_')).map_or(b.len(), |k| i + k);
            push(out, &code[i..j], Class::Number);
            i = j;
            plain = i;
            continue;
        }
        if is_ident(c) && (i == 0 || !is_ident(b[i - 1])) {
            let j = b[i..].iter().position(|x| !is_ident(*x)).map_or(b.len(), |k| i + k);
            let word = &code[i..j];
            let hit = if l.any_case { l.keywords.iter().any(|k| k.eq_ignore_ascii_case(word)) } else { l.keywords.contains(&word) };
            if hit {
                flush(out, plain, i);
                push(out, word, Class::Keyword);
                plain = j;
            }
            i = j;
            continue;
        }
        // Avance d'un caractère entier (le texte peut contenir de l'UTF-8).
        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    flush(out, plain, b.len());
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
