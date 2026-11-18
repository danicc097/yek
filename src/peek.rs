use std::path::Path;

const JSON_PEEK_LINES: usize = 20;

/// Extract an interface/signature-only view for supported file types.
///
/// Supported:
/// - C / C++ (`.c`, `.cc`, `.cpp`, `.cxx`, `.c++`, `.h`, `.hh`, `.hpp`, `.hxx`, `.h++`, `.inl`, `.ipp`)
/// - Go (`.go`)
/// - Python (`.py`, `.pyi`)
/// - JSON (`.json`): first 20 lines + `[truncated...]`
///
/// Unsupported extensions are returned unchanged.
pub fn peek_file(rel_path: &str, content: &str) -> String {
    let ext = Path::new(rel_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());

    match ext.as_deref() {
        Some("json") => peek_json(content),
        Some(
            "c" | "cc" | "cpp" | "cxx" | "c++" | "h" | "hh" | "hpp" | "hxx" | "h++" | "inl"
            | "ipp",
        ) => peek_cpp(content),
        Some("go") => peek_go(content),
        Some("py" | "pyi") => peek_python(content),
        _ => content.to_string(),
    }
}

fn peek_json(content: &str) -> String {
    let mut lines = content.lines();
    let head: Vec<&str> = lines.by_ref().take(JSON_PEEK_LINES).collect();
    if lines.next().is_some() {
        format!("{}\n[truncated...]", head.join("\n"))
    } else {
        content.to_string()
    }
}

// ============================================================================
// C / C++ / Qt interface & signature extractor
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CppTokenKind {
    LineComment,
    BlockComment,
    Preprocessor,
    Ident,
    Number,
    StringOrChar,
    Symbol,
}

#[derive(Debug, Clone, Copy)]
struct CppToken<'a> {
    kind: CppTokenKind,
    text: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CppScopeKind {
    Type,
    Enum,
    Namespace,
}

#[derive(Debug, Clone)]
struct CppScope {
    kind: CppScopeKind,
    access: &'static str,
    depth: usize,
}

const QT_BOILERPLATE_MACROS: &[&str] = &[
    "Q_OBJECT",
    "Q_GADGET",
    "Q_NAMESPACE",
    "Q_PROPERTY",
    "Q_CLASSINFO",
    "Q_DECLARE_METATYPE",
    "Q_DECLARE_INTERFACE",
    "Q_ENUM",
    "Q_ENUM_NS",
    "Q_FLAG",
    "Q_FLAGS",
    "Q_DISABLE_COPY",
    "Q_DISABLE_COPY_MOVE",
];

fn is_qt_boilerplate(ident: &str) -> bool {
    QT_BOILERPLATE_MACROS.contains(&ident)
}

fn strip_cpp_include_boilerplate(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut guard_name: Option<String> = None;

    for line in &lines {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('#') {
            let rest = rest.trim_start();
            if let Some(macro_rest) = rest.strip_prefix("ifndef") {
                if macro_rest.starts_with(char::is_whitespace) {
                    let name = macro_rest.trim();
                    if !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && (name.ends_with("_H")
                            || name.ends_with("_HPP")
                            || name.ends_with("_HXX")
                            || name.ends_with("_HH")
                            || name.ends_with("_H_")
                            || name.ends_with("_HPP_")
                            || name.ends_with("_HXX_")
                            || name.ends_with("_INCLUDED")
                            || name.ends_with("_H_INCLUDED"))
                    {
                        guard_name = Some(name.to_string());
                        break;
                    }
                }
            }
        }
    }

    let mut guard_endif_index: Option<usize> = None;
    if guard_name.is_some() {
        for (idx, line) in lines.iter().enumerate().rev() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("#endif") {
                guard_endif_index = Some(idx);
            }
            break;
        }
    }

    let mut kept = Vec::with_capacity(lines.len());
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == "#pragma once" || trimmed.starts_with("#include") {
            continue;
        }
        if let Some(ref guard) = guard_name {
            if is_guard_directive(trimmed, guard) || Some(idx) == guard_endif_index {
                continue;
            }
        }
        kept.push(*line);
    }

    kept.join("\n")
}

fn is_guard_directive(trimmed: &str, guard: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix('#') else {
        return false;
    };
    let rest = rest.trim_start();
    for kw in ["ifndef", "define"] {
        if let Some(after) = rest.strip_prefix(kw) {
            if after.starts_with(char::is_whitespace) && after.trim() == guard {
                return true;
            }
        }
    }
    false
}

fn tokenize_cpp(code: &str) -> Vec<CppToken<'_>> {
    let mut tokens = Vec::new();
    let bytes = code.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Line comment
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            let start = i;
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            tokens.push(CppToken {
                kind: CppTokenKind::LineComment,
                text: code[start..i].trim_end_matches('\r'),
            });
            continue;
        }

        // Block comment
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let start = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(len);
            tokens.push(CppToken {
                kind: CppTokenKind::BlockComment,
                text: &code[start..i],
            });
            continue;
        }

        // Preprocessor directive (handling line continuations)
        if b == b'#' {
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\n' {
                    let mut prev = i;
                    if prev > start && bytes[prev - 1] == b'\r' {
                        prev -= 1;
                    }
                    if prev > start && bytes[prev - 1] == b'\\' {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            tokens.push(CppToken {
                kind: CppTokenKind::Preprocessor,
                text: code[start..i].trim_end_matches('\r'),
            });
            continue;
        }

        // C++11 raw string literal: R"delim(...)delim"
        if b == b'R' && bytes.get(i + 1) == Some(&b'"') {
            if let Some(end_idx) = scan_cpp_raw_string(bytes, i + 2) {
                tokens.push(CppToken {
                    kind: CppTokenKind::StringOrChar,
                    text: &code[i..end_idx],
                });
                i = end_idx;
                continue;
            }
        }

        // String literal
        if b == b'"' {
            let start = i;
            i = scan_quoted_literal(bytes, i, b'"');
            tokens.push(CppToken {
                kind: CppTokenKind::StringOrChar,
                text: &code[start..i],
            });
            continue;
        }

        // Char literal
        if b == b'\'' {
            let start = i;
            i = scan_quoted_literal(bytes, i, b'\'');
            tokens.push(CppToken {
                kind: CppTokenKind::StringOrChar,
                text: &code[start..i],
            });
            continue;
        }

        // Number literal (supports C++14 digit separators like 1'000'000)
        if b.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < len {
                let c = bytes[i];
                if c.is_ascii_alphanumeric() || c == b'_' || c == b'\'' || c == b'.' {
                    if (c == b'e' || c == b'E')
                        && matches!(bytes.get(i + 1), Some(b'+') | Some(b'-'))
                    {
                        i += 2;
                        continue;
                    }
                    i += 1;
                } else {
                    break;
                }
            }
            tokens.push(CppToken {
                kind: CppTokenKind::Number,
                text: &code[start..i],
            });
            continue;
        }

        // Identifier (ASCII or UTF-8)
        let ch = code[i..].chars().next().unwrap();
        if ch.is_alphabetic() || ch == '_' {
            let start = i;
            i += ch.len_utf8();
            while i < len {
                let next_ch = code[i..].chars().next().unwrap();
                if next_ch.is_alphanumeric() || next_ch == '_' {
                    i += next_ch.len_utf8();
                } else {
                    break;
                }
            }
            let ident = &code[start..i];
            if is_qt_boilerplate(ident) {
                // Skip optional balanced (...) following Qt macro
                let mut look = i;
                while look < len && bytes[look].is_ascii_whitespace() {
                    look += 1;
                }
                if look < len && bytes[look] == b'(' {
                    i = skip_balanced_parens(bytes, look);
                }
                continue;
            }
            tokens.push(CppToken {
                kind: CppTokenKind::Ident,
                text: ident,
            });
            continue;
        }

        // Multi-char symbols (note: keep '<' and '>' single-char for templates)
        if i + 1 < len {
            let two = &code[i..i + 2];
            if matches!(
                two,
                "::" | "->" | "<=" | ">=" | "==" | "!=" | "&&" | "||" | "++" | "--"
            ) {
                tokens.push(CppToken {
                    kind: CppTokenKind::Symbol,
                    text: two,
                });
                i += 2;
                continue;
            }
        }

        let sym_end = i + ch.len_utf8();
        tokens.push(CppToken {
            kind: CppTokenKind::Symbol,
            text: &code[i..sym_end],
        });
        i = sym_end;
    }

    tokens
}

fn scan_cpp_raw_string(bytes: &[u8], delim_start: usize) -> Option<usize> {
    let mut j = delim_start;
    while j < bytes.len() && j - delim_start <= 16 {
        let c = bytes[j];
        if c == b'(' {
            let delim = &bytes[delim_start..j];
            let mut k = j + 1;
            while k < bytes.len() {
                if bytes[k] == b')'
                    && k + 1 + delim.len() < bytes.len()
                    && &bytes[k + 1..k + 1 + delim.len()] == delim
                    && bytes[k + 1 + delim.len()] == b'"'
                {
                    return Some(k + 1 + delim.len() + 1);
                }
                k += 1;
            }
            return None;
        }
        if c.is_ascii_whitespace() || c == b')' || c == b'\\' {
            return None;
        }
        j += 1;
    }
    None
}

fn scan_quoted_literal(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn skip_balanced_parens(bytes: &[u8], open_idx: usize) -> usize {
    let mut depth = 0usize;
    let mut i = open_idx;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + 1;
                }
            }
            b'"' => {
                i = scan_quoted_literal(bytes, i, b'"');
                continue;
            }
            b'\'' => {
                i = scan_quoted_literal(bytes, i, b'\'');
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    bytes.len()
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn append_cpp_token(statement: &mut Vec<String>, value: &str) {
    if let Some(prev) = statement.last().cloned() {
        let first = value.chars().next().unwrap_or(' ');
        let last = prev.chars().last().unwrap_or(' ');
        let no_space = prev == "::"
            || value == "::"
            || prev == "->"
            || value == "->"
            || prev == "."
            || value == "."
            || value == ","
            || value == ";"
            || value == ")"
            || value == "]"
            || value == "["
            || prev == "("
            || prev == "["
            || prev == "~"
            || prev == "!";
        if prev == ">" && (is_word_char(first) || value == "{") {
            statement.push(" ".to_string());
        } else if prev == "template" && value == "<" {
            statement.push(" ".to_string());
        } else if !no_space {
            if value == "=" || prev == "=" || value == ":" || prev == ":" {
                statement.push(" ".to_string());
            } else if prev == "," {
                statement.push(" ".to_string());
            } else if is_word_char(first) {
                if is_word_char(last) || matches!(last, ')' | '>' | ']' | '*' | '&') {
                    statement.push(" ".to_string());
                }
            } else if last == '>' && first == '<' {
                statement.push(" ".to_string());
            } else if matches!(first, '&' | '*') && (is_word_char(last) || last == '>') {
                // Keep `const Foo&` attached to type, separate `*` / `&` only before identifier
            } else if value == "(" && matches!(prev.as_str(), "return" | "operator") {
                statement.push(" ".to_string());
            }
        }
    }
    statement.push(value.to_string());
}

fn join_cpp_statement(statement: &[String]) -> String {
    statement.concat().trim().to_string()
}

fn cpp_statement_type_kind(statement: &[String]) -> Option<CppScopeKind> {
    let mut paren_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut found_kind: Option<CppScopeKind> = None;
    let mut has_paren_at_zero = false;
    let mut saw_arrow_after_paren = false;

    let mut prev_non_space = "";
    let mut last_non_space = "";
    let mut idx = 0;
    while idx < statement.len() {
        let tok = statement[idx].as_str();
        if tok == " " {
            idx += 1;
            continue;
        }
        match tok {
            "(" => {
                if paren_depth == 0 && angle_depth == 0 {
                    has_paren_at_zero = true;
                }
                paren_depth += 1;
            }
            ")" => {
                paren_depth = paren_depth.saturating_sub(1);
            }
            "<" if paren_depth == 0 => {
                angle_depth += 1;
            }
            ">" if paren_depth == 0 => {
                angle_depth = angle_depth.saturating_sub(1);
            }
            "->" if paren_depth == 0 && angle_depth == 0 && has_paren_at_zero => {
                saw_arrow_after_paren = true;
            }
            _ if paren_depth == 0 && angle_depth == 0 => {
                if found_kind.is_none() {
                    match tok {
                        "enum" => found_kind = Some(CppScopeKind::Enum),
                        "namespace" => found_kind = Some(CppScopeKind::Namespace),
                        "class" | "struct" | "union" => found_kind = Some(CppScopeKind::Type),
                        _ if prev_non_space == "extern" && tok.starts_with('"') => {
                            found_kind = Some(CppScopeKind::Namespace);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        prev_non_space = last_non_space;
        last_non_space = tok;
        idx += 1;
    }

    let kind = found_kind?;
    if saw_arrow_after_paren {
        return None;
    }

    // Exclude functions returning a `struct`/`enum`/`class` pointer or value
    if has_paren_at_zero {
        let joined = join_cpp_statement(statement);
        let trimmed = joined.trim_end();
        if ends_with_func_qualifiers(trimmed) {
            return None;
        }
    }

    Some(kind)
}

fn ends_with_func_qualifiers(trimmed: &str) -> bool {
    let mut s = trimmed;
    loop {
        let before = s;
        for kw in ["const", "volatile", "noexcept", "override", "final", "mutable", "&&", "&"] {
            if let Some(rest) = s.strip_suffix(kw) {
                let is_word_kw = kw.chars().all(|c| c.is_alphanumeric());
                if !is_word_kw
                    || rest.is_empty()
                    || !rest.chars().last().unwrap().is_alphanumeric()
                {
                    s = rest.trim_end();
                    break;
                }
            }
        }
        if s == before {
            break;
        }
    }
    s.ends_with(')')
}

fn cpp_paren_balance(statement: &[String]) -> usize {
    let mut bal = 0usize;
    for tok in statement {
        if tok == "(" {
            bal += 1;
        } else if tok == ")" {
            bal = bal.saturating_sub(1);
        }
    }
    bal
}

fn cpp_has_top_level_equals(statement: &[String]) -> bool {
    let mut paren_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut prev = "";
    for tok in statement {
        if tok == " " {
            continue;
        }
        match tok.as_str() {
            "(" => paren_depth += 1,
            ")" => paren_depth = paren_depth.saturating_sub(1),
            "<" if paren_depth == 0 => angle_depth += 1,
            ">" if paren_depth == 0 => angle_depth = angle_depth.saturating_sub(1),
            "=" if paren_depth == 0 && angle_depth == 0 && prev != "operator" => return true,
            _ => {}
        }
        prev = tok.as_str();
    }
    false
}

fn cpp_has_parens(statement: &[String]) -> bool {
    statement.iter().any(|t| t == "(")
}

fn cpp_default_access(statement: &[String]) -> &'static str {
    let mut angle_depth = 0usize;
    let mut paren_depth = 0usize;
    for tok in statement {
        match tok.as_str() {
            "(" => paren_depth += 1,
            ")" => paren_depth = paren_depth.saturating_sub(1),
            "<" if paren_depth == 0 => angle_depth += 1,
            ">" if paren_depth == 0 => angle_depth = angle_depth.saturating_sub(1),
            "class" if paren_depth == 0 && angle_depth == 0 => return "private",
            _ => {}
        }
    }
    "public"
}

fn parse_access_label(candidate: &str) -> Option<&'static str> {
    match candidate {
        "public" => Some("public"),
        "protected" => Some("protected"),
        "private" => Some("private"),
        "signals" | "Q_SIGNALS" => Some("signals"),
        "public slots" | "public Q_SLOTS" | "Q_SLOTS" => Some("public slots"),
        "protected slots" | "protected Q_SLOTS" => Some("protected slots"),
        "private slots" | "private Q_SLOTS" => Some("private slots"),
        _ => None,
    }
}

fn cpp_can_be_ctor_init(statement: &[String]) -> bool {
    if cpp_paren_balance(statement) != 0
        || cpp_statement_type_kind(statement).is_some()
        || cpp_has_top_level_equals(statement)
        || !cpp_has_parens(statement)
    {
        return false;
    }
    let mut i = statement.len();
    while i > 0 {
        i -= 1;
        let tok = statement[i].as_str();
        if tok == " " {
            continue;
        }
        if tok == ")" {
            return true;
        }
        if matches!(
            tok,
            "noexcept" | "const" | "volatile" | "override" | "final" | "mutable"
        ) {
            continue;
        }
        return false;
    }
    false
}

fn flush_cpp_comments(comments: &mut Vec<String>, output: &mut Vec<String>) {
    if !comments.is_empty() {
        output.push(format!("\n{}", comments.join("\n")));
        comments.clear();
    }
}

pub(crate) fn peek_cpp(content: &str) -> String {
    let cleaned = strip_cpp_include_boilerplate(content);
    let tokens = tokenize_cpp(&cleaned);

    let mut output: Vec<String> = Vec::new();
    let mut statement: Vec<String> = Vec::new();
    let mut comments: Vec<String> = Vec::new();
    let mut scopes: Vec<CppScope> = Vec::new();
    let mut brace_depth = 0usize;
    let mut function_depth = 0usize;
    let mut initializer_depth = 0usize;
    let mut emit_function_semicolon = true;
    let mut ctor_init_mode = false;
    let mut ctor_last_was_ident_or_gt = false;
    let mut ctor_paren_depth = 0usize;

    // Enum accumulator: (text, trailing_comment, leading_comment)
    let mut enum_items: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    let mut enum_tokens: Vec<String> = Vec::new();
    let mut enum_trailing: Option<String> = None;
    let mut enum_leading: Option<String> = None;

    for tok in tokens {
        let in_enum = scopes.last().is_some_and(|s| s.kind == CppScopeKind::Enum);
        if in_enum {
            match tok.kind {
                CppTokenKind::LineComment | CppTokenKind::BlockComment => {
                    let comment_text = tok.text.trim_end();
                    let is_trailing = comment_text.starts_with("///<")
                        || comment_text.starts_with("//<")
                        || (!enum_tokens.is_empty() && enum_trailing.is_none());
                    if is_trailing
                        && enum_items.last().is_some_and(|item| item.1.is_none())
                    {
                        if let Some(last) = enum_items.last_mut() {
                            last.1 = Some(comment_text.to_string());
                        }
                    } else if !enum_tokens.is_empty() {
                        enum_trailing = Some(comment_text.to_string());
                    } else {
                        enum_leading = Some(comment_text.to_string());
                    }
                }
                _ if tok.text == "}" => {
                    let item_str = join_cpp_statement(&enum_tokens);
                    if !item_str.is_empty() {
                        enum_items.push((item_str, enum_trailing.take(), enum_leading.take()));
                        enum_tokens.clear();
                    }
                    for (text, trailing, leading) in enum_items.drain(..) {
                        if let Some(l) = leading {
                            output.push(format!("  {l}"));
                        }
                        if let Some(t) = trailing {
                            output.push(format!("  {text} {t}"));
                        } else {
                            output.push(format!("  {text}"));
                        }
                    }
                    brace_depth = brace_depth.saturating_sub(1);
                    if scopes.last().is_some_and(|s| s.depth == brace_depth) {
                        scopes.pop();
                    }
                    output.push("}".to_string());
                }
                _ if tok.text == "," => {
                    enum_tokens.push(",".to_string());
                    let item_str = join_cpp_statement(&enum_tokens);
                    if !item_str.is_empty() {
                        enum_items.push((item_str, enum_trailing.take(), enum_leading.take()));
                        enum_tokens.clear();
                    }
                }
                _ => {
                    append_cpp_token(&mut enum_tokens, tok.text);
                }
            }
            continue;
        }

        if matches!(
            tok.kind,
            CppTokenKind::LineComment | CppTokenKind::BlockComment
        ) {
            if function_depth == 0 && initializer_depth == 0 && !ctor_init_mode {
                comments.push(tok.text.trim_end().to_string());
            }
            continue;
        }

        if tok.kind == CppTokenKind::Preprocessor {
            if function_depth == 0 && initializer_depth == 0 && !ctor_init_mode {
                output.push(tok.text.trim_end().to_string());
            }
            continue;
        }

        if initializer_depth > 0 {
            if tok.text == "{" {
                initializer_depth += 1;
            } else if tok.text == "}" {
                initializer_depth -= 1;
            }
            continue;
        }

        if function_depth > 0 {
            if tok.text == "{" {
                function_depth += 1;
            } else if tok.text == "}" {
                function_depth -= 1;
                if function_depth == 0 {
                    if emit_function_semicolon {
                        output.push(";".to_string());
                    }
                    statement.clear();
                    comments.clear();
                }
            }
            continue;
        }

        if ctor_init_mode {
            if tok.text == "(" {
                ctor_paren_depth += 1;
                ctor_last_was_ident_or_gt = false;
            } else if tok.text == ")" {
                ctor_paren_depth = ctor_paren_depth.saturating_sub(1);
                ctor_last_was_ident_or_gt = false;
            } else if ctor_paren_depth == 0 && tok.text == "{" {
                if ctor_last_was_ident_or_gt {
                    initializer_depth = 1;
                    ctor_last_was_ident_or_gt = false;
                } else {
                    ctor_init_mode = false;
                    let is_priv = scopes
                        .last()
                        .is_some_and(|s| matches!(s.access, "private" | "private slots"));
                    if is_priv && comments.is_empty() {
                        emit_function_semicolon = false;
                    } else {
                        emit_function_semicolon = true;
                        flush_cpp_comments(&mut comments, &mut output);
                        output.push(join_cpp_statement(&statement));
                    }
                    statement.clear();
                    function_depth = 1;
                }
            } else if ctor_paren_depth == 0 {
                ctor_last_was_ident_or_gt =
                    tok.kind == CppTokenKind::Ident || tok.text == ">";
            }
            continue;
        }

        let current_access = scopes.last().map(|s| s.access).unwrap_or("public");
        let is_private_scope = matches!(current_access, "private" | "private slots");

        match tok.text {
            "{" => {
                if let Some(kind) = cpp_statement_type_kind(&statement) {
                    if is_private_scope && comments.is_empty() {
                        emit_function_semicolon = false;
                        statement.clear();
                        function_depth = 1;
                        continue;
                    }
                    let default_acc = match kind {
                        CppScopeKind::Enum | CppScopeKind::Namespace => "public",
                        CppScopeKind::Type => cpp_default_access(&statement),
                    };
                    let stmt = join_cpp_statement(&statement);
                    scopes.push(CppScope {
                        kind,
                        access: default_acc,
                        depth: brace_depth,
                    });
                    brace_depth += 1;
                    flush_cpp_comments(&mut comments, &mut output);
                    output.push(format!("{stmt} {{"));
                    statement.clear();
                } else if cpp_paren_balance(&statement) > 0 {
                    statement.push("{}".to_string());
                    initializer_depth = 1;
                } else if cpp_has_top_level_equals(&statement) || !cpp_has_parens(&statement) {
                    statement.push(" {...}".to_string());
                    initializer_depth = 1;
                } else {
                    if is_private_scope && comments.is_empty() {
                        emit_function_semicolon = false;
                    } else {
                        emit_function_semicolon = true;
                        flush_cpp_comments(&mut comments, &mut output);
                        output.push(join_cpp_statement(&statement));
                    }
                    statement.clear();
                    function_depth = 1;
                }
            }
            "}" => {
                brace_depth = brace_depth.saturating_sub(1);
                if scopes.last().is_some_and(|s| s.depth == brace_depth) {
                    scopes.pop();
                }
                let stmt = join_cpp_statement(&statement);
                if !stmt.is_empty() {
                    flush_cpp_comments(&mut comments, &mut output);
                    output.push(stmt);
                    statement.clear();
                }
                output.push("}".to_string());
            }
            ":" => {
                let candidate = join_cpp_statement(&statement);
                if let Some(label) = parse_access_label(&candidate) {
                    if let Some(scope) = scopes.last_mut() {
                        if scope.kind == CppScopeKind::Type {
                            scope.access = label;
                            output.push(format!("\n{label}:"));
                            statement.clear();
                            comments.clear();
                            continue;
                        }
                    }
                }

                // Detect constructor member-initializer list: `Foo::Foo(...) : ... {`
                if cpp_can_be_ctor_init(&statement) {
                    ctor_init_mode = true;
                    ctor_last_was_ident_or_gt = false;
                    ctor_paren_depth = 0;
                    continue;
                }

                append_cpp_token(&mut statement, tok.text);
            }
            ";" => {
                statement.push(";".to_string());
                if is_private_scope && comments.is_empty() {
                    statement.clear();
                    continue;
                }
                flush_cpp_comments(&mut comments, &mut output);
                output.push(join_cpp_statement(&statement));
                statement.clear();
            }
            _ => {
                append_cpp_token(&mut statement, tok.text);
            }
        }
    }

    finalize_cpp_output(&output)
}

fn finalize_cpp_output(output: &[String]) -> String {
    let mut joined = String::new();
    for item in output {
        if item == ";" && !joined.is_empty() {
            joined.push(';');
            continue;
        }
        if !joined.is_empty() {
            joined.push('\n');
        }
        joined.push_str(item);
    }

    // Collapse empty access sections left behind by dropped private members.
    let lines: Vec<&str> = joined.lines().map(|l| l.trim_end()).collect();
    let mut filtered: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if is_access_label_line(trimmed) {
            // Look ahead past blank lines
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim().is_empty() {
                j += 1;
            }
            if j >= lines.len()
                || is_access_label_line(lines[j].trim())
                || lines[j].trim().starts_with('}')
            {
                i += 1;
                continue;
            }
        }
        filtered.push(lines[i]);
        i += 1;
    }

    // Collapse 3+ consecutive newlines into 2
    let mut out = String::new();
    let mut blank_run = 0usize;
    for line in filtered {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 && !out.is_empty() {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
        }
    }
    out.trim().to_string()
}

fn is_access_label_line(trimmed: &str) -> bool {
    let Some(label) = trimmed.strip_suffix(':') else {
        return false;
    };
    parse_access_label(label).is_some()
}

// ============================================================================
// Go interface / struct / signature extractor
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoTokenKind {
    Comment,
    String,
    Word,
    Punct,
    Newline,
}

#[derive(Debug, Clone, Copy)]
struct GoToken<'a> {
    kind: GoTokenKind,
    text: &'a str,
}

fn tokenize_go(code: &str) -> Vec<GoToken<'_>> {
    let mut tokens = Vec::new();
    let bytes = code.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let b = bytes[i];
        if b == b'\n' {
            tokens.push(GoToken {
                kind: GoTokenKind::Newline,
                text: "\n",
            });
            i += 1;
            continue;
        }
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Line comment
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            let start = i;
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            tokens.push(GoToken {
                kind: GoTokenKind::Comment,
                text: code[start..i].trim_end_matches('\r'),
            });
            continue;
        }

        // Block comment
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let start = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(len);
            tokens.push(GoToken {
                kind: GoTokenKind::Comment,
                text: &code[start..i],
            });
            continue;
        }

        // Raw backtick string
        if b == b'`' {
            let start = i;
            i += 1;
            while i < len && bytes[i] != b'`' {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            tokens.push(GoToken {
                kind: GoTokenKind::String,
                text: &code[start..i],
            });
            continue;
        }

        // Interpreted string / rune
        if b == b'"' || b == b'\'' {
            let start = i;
            i = scan_quoted_literal(bytes, i, b);
            tokens.push(GoToken {
                kind: GoTokenKind::String,
                text: &code[start..i],
            });
            continue;
        }

        let ch = code[i..].chars().next().unwrap();
        if ch.is_alphanumeric() || ch == '_' {
            let start = i;
            i += ch.len_utf8();
            while i < len {
                let next_ch = code[i..].chars().next().unwrap();
                if next_ch.is_alphanumeric() || next_ch == '_' || next_ch == '.' && false {
                    i += next_ch.len_utf8();
                } else {
                    break;
                }
            }
            tokens.push(GoToken {
                kind: GoTokenKind::Word,
                text: &code[start..i],
            });
            continue;
        }

        // Multi-char Go operators that are cleaner unsplit
        if i + 2 < len && &code[i..i + 3] == "..." {
            tokens.push(GoToken {
                kind: GoTokenKind::Punct,
                text: "...",
            });
            i += 3;
            continue;
        }
        if i + 1 < len {
            let two = &code[i..i + 2];
            if matches!(two, "<-" | ":=" | "==" | "!=" | "<=" | ">=" | "&&" | "||") {
                tokens.push(GoToken {
                    kind: GoTokenKind::Punct,
                    text: two,
                });
                i += 2;
                continue;
            }
        }

        let end = i + ch.len_utf8();
        tokens.push(GoToken {
            kind: GoTokenKind::Punct,
            text: &code[i..end],
        });
        i = end;
    }

    tokens
}

fn append_go_token(
    out: &mut String,
    tok: GoToken<'_>,
    prev: Option<GoToken<'_>>,
    next: Option<GoToken<'_>>,
) {
    if tok.kind == GoTokenKind::Newline {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        return;
    }

    if let Some(p) = prev {
        if p.kind != GoTokenKind::Newline && !out.ends_with('\n') && !out.ends_with(' ') {
            let need_space = if tok.kind == GoTokenKind::Comment {
                true
            } else if tok.text == "{" {
                if next.map(|n| n.text) == Some("}") {
                    false
                } else if matches!(p.text, "]" | ")") {
                    false
                } else {
                    p.text == "struct" || p.text == "interface"
                }
            } else if p.text == "," || p.text == ";" {
                true
            } else if matches!(p.text, "func" | "type" | "package" | "var" | "const" | "chan")
                && tok.text != "("
                && tok.text != "["
            {
                true
            } else if p.text == "func" && tok.text == "(" {
                true
            } else if p.text == ")"
                && (tok.kind == GoTokenKind::Word
                    || matches!(tok.text, "(" | "*" | "[" | "<-" | "{"))
            {
                true
            } else if p.text == "]" && matches!(tok.text, "struct" | "interface") {
                true
            } else if p.text == "}" && (tok.kind == GoTokenKind::Word || tok.text == "{") {
                true
            } else if matches!(tok.text, "=" | ":=" | "<-")
                || matches!(p.text, "=" | ":=" | "<-")
            {
                true
            } else if p.kind == GoTokenKind::Word && tok.kind == GoTokenKind::Word {
                true
            } else if tok.kind == GoTokenKind::String
                && (p.kind == GoTokenKind::Word
                    || p.kind == GoTokenKind::String
                    || matches!(p.text, "]" | "}" | ")" | ">"))
            {
                true
            } else if p.kind == GoTokenKind::String && tok.kind == GoTokenKind::Word {
                true
            } else {
                p.kind == GoTokenKind::Word
                    && matches!(tok.text, "*" | "[" | "<-")
                    && matches!(p.text, "return" | "chan")
                    || (p.kind == GoTokenKind::Word
                        && tok.text == "*"
                        && !matches!(p.text, "func"))
            };
            if need_space {
                out.push(' ');
            }
        }
    }

    if tok.kind == GoTokenKind::String && tok.text.starts_with('`') && tok.text.contains('\n') {
        out.push_str("`...`");
    } else {
        out.push_str(tok.text);
    }
}

pub(crate) fn peek_go(content: &str) -> String {
    let tokens = tokenize_go(content);
    let mut blocks: Vec<String> = Vec::new();
    let mut pending_comments: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < tokens.len() {
        let tok = tokens[i];
        match tok.kind {
            GoTokenKind::Newline => {
                i += 1;
            }
            GoTokenKind::Comment => {
                pending_comments.push(tok.text);
                i += 1;
                continue;
            }
            GoTokenKind::Word if tok.text == "package" => {
                let mut buf = String::new();
                if !pending_comments.is_empty() {
                    buf.push_str(&pending_comments.join("\n"));
                    buf.push('\n');
                    pending_comments.clear();
                }
                let mut prev = None;
                while i < tokens.len() {
                    let t = tokens[i];
                    i += 1;
                    if t.kind == GoTokenKind::Newline || t.text == ";" {
                        break;
                    }
                    append_go_token(&mut buf, t, prev, tokens.get(i).copied());
                    prev = Some(t);
                }
                let trimmed = buf.trim();
                if !trimmed.is_empty() {
                    blocks.push(trimmed.to_string());
                }
            }
            GoTokenKind::Word if tok.text == "import" => {
                pending_comments.clear();
                i += 1;
                while i < tokens.len() && tokens[i].kind == GoTokenKind::Comment {
                    i += 1;
                }
                if i < tokens.len() && tokens[i].text == "(" {
                    let mut pdepth = 0usize;
                    while i < tokens.len() {
                        if tokens[i].text == "(" {
                            pdepth += 1;
                        } else if tokens[i].text == ")" {
                            pdepth = pdepth.saturating_sub(1);
                            if pdepth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        i += 1;
                    }
                } else {
                    while i < tokens.len()
                        && tokens[i].kind != GoTokenKind::Newline
                        && tokens[i].text != ";"
                    {
                        i += 1;
                    }
                }
            }
            GoTokenKind::Word if tok.text == "func" => {
                let mut buf = String::new();
                if !pending_comments.is_empty() {
                    buf.push_str(&pending_comments.join("\n"));
                    buf.push('\n');
                    pending_comments.clear();
                }
                let mut paren_depth = 0usize;
                let mut bracket_depth = 0usize;
                let mut inline_brace_depth = 0usize;
                let mut expect_inline_brace = false;
                let mut prev: Option<GoToken<'_>> = None;

                while i < tokens.len() {
                    let t = tokens[i];
                    i += 1;

                    if t.kind == GoTokenKind::Comment {
                        continue;
                    }

                    if t.kind == GoTokenKind::Newline {
                        if paren_depth == 0 && bracket_depth == 0 && inline_brace_depth == 0 {
                            // Bodyless func declaration (e.g. asm/cgo stub)
                            break;
                        }
                        continue;
                    }

                    if paren_depth == 0 && bracket_depth == 0 && inline_brace_depth == 0 {
                        if t.text == "{" {
                            if expect_inline_brace {
                                inline_brace_depth = 1;
                                expect_inline_brace = false;
                                append_go_token(&mut buf, t, prev, tokens.get(i).copied());
                                prev = Some(t);
                                continue;
                            } else {
                                // Function body starts here -> skip until matching '}'
                                let mut bdepth = 1usize;
                                while i < tokens.len() && bdepth > 0 {
                                    if tokens[i].text == "{" {
                                        bdepth += 1;
                                    } else if tokens[i].text == "}" {
                                        bdepth -= 1;
                                    }
                                    i += 1;
                                }
                                break;
                            }
                        }
                        if t.text == ";" {
                            break;
                        }
                        if t.kind == GoTokenKind::Word
                            && (t.text == "struct" || t.text == "interface")
                        {
                            expect_inline_brace = true;
                        } else if t.text != "{" {
                            expect_inline_brace = false;
                        }
                    } else if inline_brace_depth > 0 {
                        if t.text == "{" {
                            inline_brace_depth += 1;
                        } else if t.text == "}" {
                            inline_brace_depth -= 1;
                        }
                    }

                    if t.text == "(" {
                        paren_depth += 1;
                    } else if t.text == ")" {
                        paren_depth = paren_depth.saturating_sub(1);
                    } else if t.text == "[" {
                        bracket_depth += 1;
                    } else if t.text == "]" {
                        bracket_depth = bracket_depth.saturating_sub(1);
                    }

                    append_go_token(&mut buf, t, prev, tokens.get(i).copied());
                    prev = Some(t);
                }

                let trimmed = buf.trim();
                if !trimmed.is_empty() {
                    blocks.push(trimmed.to_string());
                }
            }
            GoTokenKind::Word if tok.text == "type" => {
                let mut buf = String::new();
                if !pending_comments.is_empty() {
                    buf.push_str(&pending_comments.join("\n"));
                    buf.push('\n');
                    pending_comments.clear();
                }
                let mut paren_depth = 0usize;
                let mut brace_depth = 0usize;
                let mut prev: Option<GoToken<'_>> = None;

                while i < tokens.len() {
                    let t = tokens[i];
                    i += 1;

                    if t.kind == GoTokenKind::Newline {
                        if paren_depth == 0 && brace_depth == 0 {
                            break;
                        }
                        if !buf.ends_with('\n') {
                            buf.push('\n');
                            if brace_depth > 0 || paren_depth > 0 {
                                let indent = "\t".repeat(brace_depth + paren_depth);
                                buf.push_str(&indent);
                            }
                        }
                        prev = Some(t);
                        continue;
                    }

                    if t.text == "(" {
                        paren_depth += 1;
                    } else if t.text == ")" {
                        paren_depth = paren_depth.saturating_sub(1);
                    } else if t.text == "{" {
                        brace_depth += 1;
                    } else if t.text == "}" {
                        brace_depth = brace_depth.saturating_sub(1);
                        // Remove extra tab if closing brace was preceded by indent
                        if buf.ends_with('\t') {
                            buf.pop();
                        }
                    }

                    append_go_token(&mut buf, t, prev, tokens.get(i).copied());
                    prev = Some(t);
                }

                let trimmed = buf.trim();
                if !trimmed.is_empty() {
                    blocks.push(trimmed.to_string());
                }
            }
            GoTokenKind::Word if tok.text == "const" || tok.text == "var" => {
                let mut buf = String::new();
                if !pending_comments.is_empty() {
                    buf.push_str(&pending_comments.join("\n"));
                    buf.push('\n');
                    pending_comments.clear();
                }
                let mut paren_depth = 0usize;
                let mut brace_depth = 0usize;
                let mut prev: Option<GoToken<'_>> = None;

                while i < tokens.len() {
                    let t = tokens[i];
                    i += 1;

                    if t.kind == GoTokenKind::Newline {
                        if paren_depth == 0 && brace_depth == 0 {
                            break;
                        }
                        if !buf.ends_with('\n') {
                            buf.push('\n');
                            if paren_depth > 0 || brace_depth > 0 {
                                buf.push_str(&"\t".repeat(paren_depth + brace_depth));
                            }
                        }
                        prev = Some(t);
                        continue;
                    }

                    if t.text == "(" {
                        paren_depth += 1;
                    } else if t.text == ")" {
                        paren_depth = paren_depth.saturating_sub(1);
                        if buf.ends_with('\t') {
                            buf.pop();
                        }
                    } else if t.text == "{" {
                        brace_depth += 1;
                        // Collapse composite literal or function literal bodies in const/var
                        let mut bdepth = 1usize;
                        while i < tokens.len() && bdepth > 0 {
                            if tokens[i].text == "{" {
                                bdepth += 1;
                            } else if tokens[i].text == "}" {
                                bdepth -= 1;
                            }
                            i += 1;
                        }
                        brace_depth = brace_depth.saturating_sub(1);
                        if buf.ends_with(' ') {
                            buf.pop();
                        }
                        buf.push('{');
                        buf.push_str("...}");
                        prev = Some(GoToken {
                            kind: GoTokenKind::Punct,
                            text: "}",
                        });
                        continue;
                    } else if t.text == "}" {
                        brace_depth = brace_depth.saturating_sub(1);
                    }

                    append_go_token(&mut buf, t, prev, tokens.get(i).copied());
                    prev = Some(t);
                }

                let trimmed = buf.trim();
                if !trimmed.is_empty() {
                    blocks.push(trimmed.to_string());
                }
            }
            _ => {
                if tok.kind != GoTokenKind::Comment && tok.kind != GoTokenKind::Newline {
                    pending_comments.clear();
                }
                let mut brace_depth = 0usize;
                let mut paren_depth = 0usize;
                while i < tokens.len() {
                    let t = tokens[i];
                    i += 1;
                    if t.text == "{" {
                        brace_depth += 1;
                    } else if t.text == "}" {
                        brace_depth = brace_depth.saturating_sub(1);
                    } else if t.text == "(" {
                        paren_depth += 1;
                    } else if t.text == ")" {
                        paren_depth = paren_depth.saturating_sub(1);
                    } else if t.kind == GoTokenKind::Newline && brace_depth == 0 && paren_depth == 0
                    {
                        break;
                    }
                }
            }
        }
    }

    blocks.join("\n\n")
}

// ============================================================================
// Python class / decorator / signature extractor
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PyStringState {
    Single,
    Double,
    TripleSingle,
    TripleDouble,
}

#[derive(Debug, Clone)]
enum PyScope {
    Class {
        indent: usize,
        indent_str: String,
        has_emitted_body: bool,
    },
    Def {
        indent: usize,
    },
}

impl PyScope {
    fn indent(&self) -> usize {
        match self {
            PyScope::Class { indent, .. } | PyScope::Def { indent } => *indent,
        }
    }
}

fn scan_python_line(
    line: &str,
    in_string: &mut Option<PyStringState>,
    bracket_depth: &mut usize,
) -> bool {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut last_non_ws_before_comment: Option<u8> = None;

    while i < len {
        if let Some(state) = *in_string {
            match state {
                PyStringState::Single | PyStringState::Double => {
                    let q = if state == PyStringState::Single {
                        b'\''
                    } else {
                        b'"'
                    };
                    if bytes[i] == b'\\' {
                        i = (i + 2).min(len);
                        continue;
                    }
                    if bytes[i] == q {
                        *in_string = None;
                        last_non_ws_before_comment = Some(q);
                    }
                    i += 1;
                }
                PyStringState::TripleSingle | PyStringState::TripleDouble => {
                    let q = if state == PyStringState::TripleSingle {
                        b'\''
                    } else {
                        b'"'
                    };
                    if bytes[i] == b'\\' {
                        i = (i + 2).min(len);
                        continue;
                    }
                    if i + 2 < len && bytes[i] == q && bytes[i + 1] == q && bytes[i + 2] == q {
                        *in_string = None;
                        last_non_ws_before_comment = Some(q);
                        i += 3;
                    } else {
                        i += 1;
                    }
                }
            }
            continue;
        }

        let b = bytes[i];
        if b == b'#' {
            break;
        }
        if b == b'\'' || b == b'"' {
            if i + 2 < len && bytes[i + 1] == b && bytes[i + 2] == b {
                *in_string = Some(if b == b'\'' {
                    PyStringState::TripleSingle
                } else {
                    PyStringState::TripleDouble
                });
                i += 3;
            } else {
                *in_string = Some(if b == b'\'' {
                    PyStringState::Single
                } else {
                    PyStringState::Double
                });
                i += 1;
            }
            last_non_ws_before_comment = Some(b);
            continue;
        }

        if matches!(b, b'(' | b'[' | b'{') {
            *bracket_depth += 1;
        } else if matches!(b, b')' | b']' | b'}') {
            *bracket_depth = bracket_depth.saturating_sub(1);
        }

        if !b.is_ascii_whitespace() {
            last_non_ws_before_comment = Some(b);
        }
        i += 1;
    }

    in_string.is_some() || last_non_ws_before_comment == Some(b'\\')
}

fn is_python_def_keyword(trimmed: &str) -> bool {
    if let Some(rest) = trimmed.strip_prefix("def") {
        return rest.starts_with(char::is_whitespace);
    }
    if let Some(rest) = trimmed.strip_prefix("async") {
        if rest.starts_with(char::is_whitespace) {
            let after_async = rest.trim_start();
            if let Some(after_def) = after_async.strip_prefix("def") {
                return after_def.starts_with(char::is_whitespace);
            }
        }
    }
    false
}

fn is_python_class_keyword(trimmed: &str) -> bool {
    if let Some(rest) = trimmed.strip_prefix("class") {
        return rest.starts_with(char::is_whitespace) || rest.starts_with('(') || rest.starts_with(':');
    }
    false
}

fn find_python_header_colon(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_string: Option<PyStringState> = None;
    let mut bracket_depth = 0usize;

    while i < len {
        if let Some(state) = in_string {
            match state {
                PyStringState::Single | PyStringState::Double => {
                    let q = if state == PyStringState::Single {
                        b'\''
                    } else {
                        b'"'
                    };
                    if bytes[i] == b'\\' {
                        i = (i + 2).min(len);
                        continue;
                    }
                    if bytes[i] == q {
                        in_string = None;
                    }
                    i += 1;
                }
                PyStringState::TripleSingle | PyStringState::TripleDouble => {
                    let q = if state == PyStringState::TripleSingle {
                        b'\''
                    } else {
                        b'"'
                    };
                    if bytes[i] == b'\\' {
                        i = (i + 2).min(len);
                        continue;
                    }
                    if i + 2 < len && bytes[i] == q && bytes[i + 1] == q && bytes[i + 2] == q {
                        in_string = None;
                        i += 3;
                    } else {
                        i += 1;
                    }
                }
            }
            continue;
        }

        let b = bytes[i];
        if b == b'#' {
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'\'' || b == b'"' {
            if i + 2 < len && bytes[i + 1] == b && bytes[i + 2] == b {
                in_string = Some(if b == b'\'' {
                    PyStringState::TripleSingle
                } else {
                    PyStringState::TripleDouble
                });
                i += 3;
            } else {
                in_string = Some(if b == b'\'' {
                    PyStringState::Single
                } else {
                    PyStringState::Double
                });
                i += 1;
            }
            continue;
        }
        if matches!(b, b'(' | b'[' | b'{') {
            bracket_depth += 1;
        } else if matches!(b, b')' | b']' | b'}') {
            bracket_depth = bracket_depth.saturating_sub(1);
        } else if b == b':' && bracket_depth == 0 {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_top_level_equals(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut depth = 0usize;
    let mut in_quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_quote {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                in_quote = None;
            }
            i += 1;
            continue;
        }
        if b == b'#' {
            break;
        }
        if b == b'\'' || b == b'"' {
            in_quote = Some(b);
        } else if matches!(b, b'(' | b'[' | b'{') {
            depth += 1;
        } else if matches!(b, b')' | b']' | b'}') {
            depth = depth.saturating_sub(1);
        } else if b == b'=' && depth == 0 && bytes.get(i + 1) != Some(&b'=') {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn pop_py_scopes(scopes: &mut Vec<PyScope>, target_indent: usize, output: &mut Vec<String>) {
    while scopes.last().is_some_and(|s| s.indent() >= target_indent) {
        if let Some(PyScope::Class {
            indent_str,
            has_emitted_body,
            ..
        }) = scopes.pop()
        {
            if !has_emitted_body {
                output.push(format!("{indent_str}    ..."));
            }
        }
    }
}

pub(crate) fn peek_python(content: &str) -> String {
    let mut output: Vec<String> = Vec::new();
    let mut scopes: Vec<PyScope> = Vec::new();
    let mut pending_comments: Vec<(usize, String)> = Vec::new();
    let mut pending_decorators: Vec<String> = Vec::new();

    let mut in_string: Option<PyStringState> = None;
    let mut bracket_depth = 0usize;
    let mut stmt_lines: Vec<&str> = Vec::new();

    let flush_statement = |stmt_lines: &mut Vec<&str>,
                           scopes: &mut Vec<PyScope>,
                           pending_comments: &mut Vec<(usize, String)>,
                           pending_decorators: &mut Vec<String>,
                           output: &mut Vec<String>| {
        if stmt_lines.is_empty() {
            return;
        }

        let first_line = stmt_lines[0];
        let trimmed = first_line.trim_start();
        let indent = first_line.len() - trimmed.len();
        let indent_str = &first_line[..indent];

        pop_py_scopes(scopes, indent, output);

        let in_def = scopes.iter().any(|s| matches!(s, PyScope::Def { .. }));
        if in_def {
            pending_comments.clear();
            pending_decorators.clear();
            stmt_lines.clear();
            return;
        }

        if trimmed.starts_with('@') {
            for l in stmt_lines.drain(..) {
                pending_decorators.push(l.trim_end().to_string());
            }
            return;
        }

        if is_python_class_keyword(trimmed) {
            if let Some(PyScope::Class {
                has_emitted_body,
                ..
            }) = scopes.last_mut()
            {
                *has_emitted_body = true;
            }
            for (c_indent, c_line) in pending_comments.drain(..) {
                if c_indent == indent {
                    output.push(c_line);
                }
            }
            for d in pending_decorators.drain(..) {
                output.push(d);
            }
            let joined = stmt_lines.join("\n");
            if let Some(colon_idx) = find_python_header_colon(&joined) {
                let header = joined[..=colon_idx].trim_end();
                output.push(header.to_string());
                let after = joined[colon_idx + 1..].trim();
                if after.is_empty() || after.starts_with('#') {
                    scopes.push(PyScope::Class {
                        indent,
                        indent_str: indent_str.to_string(),
                        has_emitted_body: false,
                    });
                } else {
                    output.push(format!("{indent_str}    ..."));
                }
            } else {
                output.push(joined.trim_end().to_string());
                scopes.push(PyScope::Class {
                    indent,
                    indent_str: indent_str.to_string(),
                    has_emitted_body: false,
                });
            }
            stmt_lines.clear();
            return;
        }

        if is_python_def_keyword(trimmed) {
            if let Some(PyScope::Class {
                has_emitted_body,
                ..
            }) = scopes.last_mut()
            {
                *has_emitted_body = true;
            }
            for (c_indent, c_line) in pending_comments.drain(..) {
                if c_indent == indent {
                    output.push(c_line);
                }
            }
            for d in pending_decorators.drain(..) {
                output.push(d);
            }
            let joined = stmt_lines.join("\n");
            if let Some(colon_idx) = find_python_header_colon(&joined) {
                let sig = joined[..=colon_idx].trim_end();
                output.push(format!("{sig} ..."));
                let after = joined[colon_idx + 1..].trim();
                if after.is_empty() || after.starts_with('#') {
                    scopes.push(PyScope::Def { indent });
                }
            } else {
                output.push(joined.trim_end().to_string());
            }
            stmt_lines.clear();
            return;
        }

        pending_decorators.clear();
        pending_comments.clear();

        if let Some(PyScope::Class {
            has_emitted_body,
            ..
        }) = scopes.last_mut()
        {
            let is_skip = trimmed == "pass"
                || trimmed == "..."
                || trimmed.starts_with("\"\"\"")
                || trimmed.starts_with("'''")
                || trimmed.starts_with('"')
                || trimmed.starts_with('\'')
                || trimmed.starts_with("import ")
                || trimmed.starts_with("from ");
            let is_control_flow = [
                "if ",
                "elif ",
                "else:",
                "for ",
                "while ",
                "try:",
                "except",
                "finally:",
                "with ",
                "match ",
                "case ",
            ]
            .iter()
            .any(|kw| trimmed.starts_with(kw));

            if !is_skip && !is_control_flow {
                *has_emitted_body = true;
                if stmt_lines.len() == 1 {
                    output.push(stmt_lines[0].trim_end().to_string());
                } else if let Some(eq_pos) = find_top_level_equals(first_line) {
                    output.push(format!("{} = ...", first_line[..eq_pos].trim_end()));
                } else {
                    for l in stmt_lines.iter() {
                        output.push(l.trim_end().to_string());
                    }
                }
            }
        } else if trimmed.starts_with("type ") && trimmed.contains('=') && stmt_lines.len() == 1 {
            output.push(stmt_lines[0].trim_end().to_string());
        }

        stmt_lines.clear();
    };

    for line in content.lines() {
        let starting_new_stmt = stmt_lines.is_empty() && in_string.is_none() && bracket_depth == 0;
        if starting_new_stmt {
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('#') {
                let c_indent = line.len() - trimmed.len();
                let active_def_indent = scopes.iter().rev().find_map(|s| match s {
                    PyScope::Def { indent } => Some(*indent),
                    _ => None,
                });
                if active_def_indent.is_none_or(|d| c_indent <= d) {
                    pending_comments.push((c_indent, line.trim_end().to_string()));
                }
                continue;
            }
        }

        let line_continues = scan_python_line(line, &mut in_string, &mut bracket_depth);
        stmt_lines.push(line);

        if !line_continues && in_string.is_none() && bracket_depth == 0 {
            flush_statement(
                &mut stmt_lines,
                &mut scopes,
                &mut pending_comments,
                &mut pending_decorators,
                &mut output,
            );
        }
    }

    flush_statement(
        &mut stmt_lines,
        &mut scopes,
        &mut pending_comments,
        &mut pending_decorators,
        &mut output,
    );
    pop_py_scopes(&mut scopes, 0, &mut output);

    output.join("\n")
}
