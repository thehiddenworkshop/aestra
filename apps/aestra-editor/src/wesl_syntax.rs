//! Lightweight WGSL/WESL tokenizer for syntax coloration (Milestone 7-3).
//!
//! Not a parser: it classifies the source into coarse token runs (comments, keywords, types,
//! numbers, attributes, strings, identifiers, punctuation) for rendering colored text spans. The
//! runs concatenate back to the exact source, so nothing is lost. Real diagnostics come from the
//! compiler, not this scanner.

/// The colour class of a token run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WeslTokenKind {
    Comment,
    Keyword,
    Type,
    Number,
    Attribute,
    String,
    Ident,
    Punctuation,
    Whitespace,
}

/// One classified run of source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WeslToken {
    pub(crate) kind: WeslTokenKind,
    pub(crate) text: String,
}

const KEYWORDS: &[&str] = &[
    "fn",
    "let",
    "var",
    "const",
    "const_assert",
    "override",
    "struct",
    "return",
    "if",
    "else",
    "for",
    "while",
    "loop",
    "break",
    "continue",
    "switch",
    "case",
    "default",
    "fallthrough",
    "discard",
    "true",
    "false",
    "alias",
    "enable",
    "requires",
    "diagnostic",
    "import",
    "export",
    "as",
    "bitcast",
    "workgroup",
    "uniform",
    "storage",
    "function",
    "private",
    "read",
    "write",
    "read_write",
];

const TYPES: &[&str] = &[
    "bool",
    "f16",
    "f32",
    "i32",
    "u32",
    "atomic",
    "array",
    "ptr",
    "sampler",
    "sampler_comparison",
    "void",
];

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

/// Whether an identifier names a builtin type (including the `vecN`/`matN`/`texture*` families).
fn is_type_ident(ident: &str) -> bool {
    TYPES.contains(&ident)
        || ident.starts_with("vec")
        || ident.starts_with("mat")
        || ident.starts_with("texture")
}

/// Classifies `source` into consecutive coloured runs. Concatenating the runs reproduces `source`.
pub(crate) fn tokenize(source: &str) -> Vec<WeslToken> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    let push = |tokens: &mut Vec<WeslToken>, kind, slice: &[char]| {
        tokens.push(WeslToken {
            kind,
            text: slice.iter().collect(),
        });
    };

    while index < chars.len() {
        let c = chars[index];
        let start = index;

        if c.is_whitespace() {
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
            push(&mut tokens, WeslTokenKind::Whitespace, &chars[start..index]);
        } else if c == '/' && chars.get(index + 1) == Some(&'/') {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            push(&mut tokens, WeslTokenKind::Comment, &chars[start..index]);
        } else if c == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            while index < chars.len()
                && !(chars[index] == '*' && chars.get(index + 1) == Some(&'/'))
            {
                index += 1;
            }
            index = (index + 2).min(chars.len());
            push(&mut tokens, WeslTokenKind::Comment, &chars[start..index]);
        } else if c == '"' {
            index += 1;
            while index < chars.len() && chars[index] != '"' {
                if chars[index] == '\\' {
                    index += 1;
                }
                index += 1;
            }
            index = (index + 1).min(chars.len());
            push(&mut tokens, WeslTokenKind::String, &chars[start..index]);
        } else if c == '@' {
            index += 1;
            while index < chars.len() && is_ident_continue(chars[index]) {
                index += 1;
            }
            push(&mut tokens, WeslTokenKind::Attribute, &chars[start..index]);
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(index + 1).is_some_and(|n| n.is_ascii_digit()))
        {
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric()
                    || chars[index] == '.'
                    || chars[index] == '_')
            {
                index += 1;
            }
            push(&mut tokens, WeslTokenKind::Number, &chars[start..index]);
        } else if is_ident_start(c) {
            while index < chars.len() && is_ident_continue(chars[index]) {
                index += 1;
            }
            let ident: String = chars[start..index].iter().collect();
            let kind = if KEYWORDS.contains(&ident.as_str()) {
                WeslTokenKind::Keyword
            } else if is_type_ident(&ident) {
                WeslTokenKind::Type
            } else {
                WeslTokenKind::Ident
            };
            push(&mut tokens, kind, &chars[start..index]);
        } else {
            index += 1;
            push(
                &mut tokens,
                WeslTokenKind::Punctuation,
                &chars[start..index],
            );
        }
    }

    tokens
}

/// The names of top-level functions declared in the source (`fn NAME(...)`), for the WESL properties
/// view. Best-effort from the tokenizer (skips comments/whitespace), not a real parser.
pub(crate) fn declared_functions(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut expecting_name = false;
    for token in tokenize(source) {
        match token.kind {
            WeslTokenKind::Whitespace | WeslTokenKind::Comment => {}
            WeslTokenKind::Keyword if token.text == "fn" => expecting_name = true,
            WeslTokenKind::Ident if expecting_name => {
                names.push(token.text);
                expecting_name = false;
            }
            _ => expecting_name = false,
        }
    }
    names
}

/// The names of the modules this source imports, from `import package::<name>::…;` statements
/// (each `<name>` is one dependency). Best-effort from the tokenizer, so `import` inside comments or
/// strings is ignored; grouped and multi-target imports contribute every `package::<name>` they
/// mention. Duplicates are removed, order preserved. Feeds the dependency graph that recompiles a
/// module's dependents when it changes (Milestone 8).
pub(crate) fn imported_modules(source: &str) -> Vec<String> {
    let tokens: Vec<WeslToken> = tokenize(source)
        .into_iter()
        .filter(|token| {
            !matches!(
                token.kind,
                WeslTokenKind::Whitespace | WeslTokenKind::Comment
            )
        })
        .collect();
    let mut modules = Vec::new();
    let mut in_import = false;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token.kind == WeslTokenKind::Keyword && token.text == "import" {
            in_import = true;
            index += 1;
            continue;
        }
        if in_import {
            if token.text == ";" {
                in_import = false;
                index += 1;
                continue;
            }
            // Match `package :: <name>` (the tokenizer emits each `:` separately).
            let is_package = token.kind == WeslTokenKind::Ident && token.text == "package";
            let colons = tokens.get(index + 1).map(|t| t.text.as_str()) == Some(":")
                && tokens.get(index + 2).map(|t| t.text.as_str()) == Some(":");
            if is_package
                && colons
                && let Some(name) = tokens.get(index + 3).filter(|t| t.kind == WeslTokenKind::Ident)
            {
                if !modules.contains(&name.text) {
                    modules.push(name.text.clone());
                }
                index += 4;
                continue;
            }
        }
        index += 1;
    }
    modules
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<(WeslTokenKind, String)> {
        tokenize(source)
            .into_iter()
            .map(|token| (token.kind, token.text))
            .collect()
    }

    #[test]
    fn imported_modules_lists_each_package_dependency_once() {
        let source = "// import package::commented::out;\n\
                      import package::noise::value_noise;\n\
                      import package::color::{grade, tint};\n\
                      import package::noise::fbm;\n\
                      fn main() {}";
        // Each distinct module named after `package::` is a dependency; `noise` appears once.
        assert_eq!(
            imported_modules(source),
            vec!["noise".to_string(), "color".to_string()]
        );
        assert!(imported_modules("fn main() {}").is_empty());
    }

    #[test]
    fn declared_functions_lists_each_fn_name_in_order() {
        let source =
            "// noise\nfn hash2(p: vec2<f32>) -> f32 { }\nfn value_noise(p: vec2<f32>) -> f32 { }";
        assert_eq!(declared_functions(source), vec!["hash2", "value_noise"]);
        assert!(declared_functions("let x = 1;").is_empty());
    }

    #[test]
    fn runs_concatenate_back_to_the_source() {
        let source = "fn main(uv: vec2<f32>) -> f32 {\n  // comment\n  return 1.0;\n}";
        let rebuilt: String = tokenize(source)
            .into_iter()
            .map(|token| token.text)
            .collect();
        assert_eq!(rebuilt, source);
    }

    #[test]
    fn classifies_keywords_types_numbers_and_comments() {
        let tokens = kinds("fn f() -> f32 { return 1.0; } // done");
        assert!(tokens.contains(&(WeslTokenKind::Keyword, "fn".into())));
        assert!(tokens.contains(&(WeslTokenKind::Keyword, "return".into())));
        assert!(tokens.contains(&(WeslTokenKind::Type, "f32".into())));
        assert!(tokens.contains(&(WeslTokenKind::Number, "1.0".into())));
        assert!(tokens.contains(&(WeslTokenKind::Comment, "// done".into())));
        // vecN / matN families and attributes are recognised too.
        assert!(kinds("vec3<f32>").contains(&(WeslTokenKind::Type, "vec3".into())));
        assert!(kinds("@group(0)").contains(&(WeslTokenKind::Attribute, "@group".into())));
    }

    #[test]
    fn block_comments_and_unterminated_input_do_not_panic() {
        assert_eq!(
            kinds("/* a */x"),
            vec![
                (WeslTokenKind::Comment, "/* a */".into()),
                (WeslTokenKind::Ident, "x".into()),
            ]
        );
        // Unterminated block comment and string consume to the end without panicking.
        let _ = tokenize("/* unterminated");
        let _ = tokenize("\"unterminated");
    }
}
