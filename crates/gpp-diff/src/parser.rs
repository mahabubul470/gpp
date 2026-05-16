//! Generic declaration extraction driven by a [`LanguageParser`]'s query.
//!
//! Each declaration gets two fingerprints:
//!
//! * `full_fp` — hash of the normalized declaration text. Changes whenever
//!   anything (signature, body, or name) changes.
//! * `body_fp` — same, but with the name token blanked out. Two declarations
//!   with equal `body_fp` but different names are a rename; equal `body_fp`
//!   in different files is a move.
//!
//! Normalization trims trailing whitespace and blank edges so cosmetic
//! reformatting does not look like a semantic change.

use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

use crate::error::{Error, Result};
use crate::lang::LanguageParser;

/// A single top-level (or nested) declaration found in a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    /// Friendly kind: `fn`, `struct`, `enum`, `trait`, `class`, `type`, …
    pub kind: String,
    /// The declared identifier.
    pub name: String,
    /// 1-based inclusive line span in the source.
    pub start_line: usize,
    pub end_line: usize,
    /// Fingerprint of the whole declaration (name included).
    pub full_fp: String,
    /// Fingerprint with the name blanked — stable across renames/moves.
    pub body_fp: String,
}

const NAME_HOLE: &str = "\u{0}\u{0}NAME\u{0}\u{0}";

fn fingerprint(text: &str) -> String {
    let mut norm = String::with_capacity(text.len());
    for line in text.lines() {
        norm.push_str(line.trim_end());
        norm.push('\n');
    }
    let trimmed = norm.trim();
    blake3::hash(trimmed.as_bytes()).to_hex()[..16].to_string()
}

/// Friendly label for a grammar node kind.
fn friendly_kind(node_kind: &str) -> String {
    let k = match node_kind {
        "function_item" | "function_definition" | "function_declaration" => "fn",
        "method_declaration" | "method_definition" => "method",
        "struct_item" => "struct",
        "enum_item" | "enum_declaration" => "enum",
        "union_item" => "union",
        "trait_item" | "interface_declaration" => "trait",
        "impl_item" => "impl",
        "mod_item" => "mod",
        "class_definition" | "class_declaration" => "class",
        "type_item" | "type_alias_declaration" | "type_declaration" => "type",
        "const_item" => "const",
        "static_item" => "static",
        "macro_definition" => "macro",
        other => other,
    };
    k.to_string()
}

/// Parse `source` and return its declarations, sorted by start line.
///
/// `source` must be UTF-8. An empty result is valid (file with no
/// top-level declarations); a grammar/query failure is an error.
pub fn parse_declarations(parser: &dyn LanguageParser, source: &[u8]) -> Result<Vec<Declaration>> {
    let src = std::str::from_utf8(source).map_err(|_| Error::NotUtf8)?;
    let ts_lang = parser.ts_language()?;

    let mut ts = Parser::new();
    ts.set_language(&ts_lang)
        .map_err(|e| Error::LanguageLoad(e.to_string()))?;
    let tree = ts.parse(src, None).ok_or(Error::ParseFailed)?;

    let query = Query::new(&ts_lang, parser.declaration_query()).map_err(|e| Error::BadQuery {
        language: parser.language().name().to_string(),
        message: e.to_string(),
    })?;
    let decl_idx = query.capture_index_for_name("decl");
    let name_idx = query.capture_index_for_name("name");
    let (Some(decl_idx), Some(name_idx)) = (decl_idx, name_idx) else {
        return Err(Error::BadQuery {
            language: parser.language().name().to_string(),
            message: "query must bind @decl and @name".into(),
        });
    };

    let mut out = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut it = cursor.matches(&query, tree.root_node(), src.as_bytes());
    while let Some(m) = it.next() {
        let mut decl_node = None;
        let mut name_node = None;
        for cap in m.captures {
            if cap.index == decl_idx {
                decl_node = Some(cap.node);
            } else if cap.index == name_idx {
                name_node = Some(cap.node);
            }
        }
        let (Some(decl), Some(name)) = (decl_node, name_node) else {
            continue;
        };

        let d_start = decl.start_byte();
        let d_end = decl.end_byte();
        let decl_text = &src[d_start..d_end];
        let name_text = src[name.start_byte()..name.end_byte()].to_string();

        // Build the name-blanked variant for the body fingerprint.
        let n_rel_start = name.start_byte().saturating_sub(d_start);
        let n_rel_end = name.end_byte().saturating_sub(d_start);
        let mut blanked = String::with_capacity(decl_text.len());
        blanked.push_str(&decl_text[..n_rel_start.min(decl_text.len())]);
        blanked.push_str(NAME_HOLE);
        if n_rel_end <= decl_text.len() {
            blanked.push_str(&decl_text[n_rel_end..]);
        }

        out.push(Declaration {
            kind: friendly_kind(decl.kind()),
            name: name_text,
            start_line: decl.start_position().row + 1,
            end_line: decl.end_position().row + 1,
            full_fp: fingerprint(decl_text),
            body_fp: fingerprint(&blanked),
        });
    }

    out.sort_by_key(|d| d.start_line);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::{Language, parser_for};

    fn decls(lang: Language, src: &str) -> Vec<Declaration> {
        let p = parser_for(lang).unwrap();
        parse_declarations(p.as_ref(), src.as_bytes()).unwrap()
    }

    #[test]
    fn rust_extracts_fns_and_types() {
        let d = decls(
            Language::Rust,
            "fn alpha() {}\nstruct Beta { x: u8 }\nfn gamma(n: u8) -> u8 { n }\n",
        );
        let names: Vec<_> = d.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Beta", "gamma"]);
        assert_eq!(d[0].kind, "fn");
        assert_eq!(d[1].kind, "struct");
    }

    #[test]
    fn rename_keeps_body_fp() {
        let a = decls(Language::Rust, "fn old(n: u8) -> u8 { n + 1 }\n");
        let b = decls(Language::Rust, "fn renamed(n: u8) -> u8 { n + 1 }\n");
        assert_ne!(a[0].full_fp, b[0].full_fp);
        assert_eq!(a[0].body_fp, b[0].body_fp);
    }

    #[test]
    fn whitespace_only_change_is_stable() {
        let a = decls(Language::Python, "def f(x):\n    return x\n");
        let b = decls(Language::Python, "def f(x):\n    return x   \n\n");
        assert_eq!(a[0].full_fp, b[0].full_fp);
    }

    #[test]
    fn go_and_ts_parse() {
        assert_eq!(
            decls(
                Language::Go,
                "package m\nfunc Hello() string { return \"hi\" }\n"
            )[0]
            .name,
            "Hello"
        );
        assert_eq!(
            decls(
                Language::TypeScript,
                "export class Widget { render() {} }\n"
            )[0]
            .name,
            "Widget"
        );
    }
}
