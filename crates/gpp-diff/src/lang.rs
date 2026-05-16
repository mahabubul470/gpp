//! Language detection and the pluggable [`LanguageParser`] interface.
//!
//! A parser supplies two things: the tree-sitter grammar, and a query that
//! captures declarations. Every match must bind `@decl` (the declaration
//! node, used for the body fingerprint) and `@name` (its identifier). The
//! generic extractor in [`crate::parser`] does the rest, so adding a language
//! is just a grammar + a query.

use crate::error::{Error, Result};

/// A source language gpp can diff structurally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    Go,
}

impl Language {
    pub fn name(self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::Python => "python",
            Language::TypeScript => "typescript",
            Language::Go => "go",
        }
    }
}

/// Map a file path to a [`Language`] by extension, if supported.
pub fn detect_language(path: &str) -> Option<Language> {
    let ext = path.rsplit('.').next()?;
    Some(match ext {
        "rs" => Language::Rust,
        "py" | "pyi" => Language::Python,
        "ts" | "tsx" | "mts" | "cts" => Language::TypeScript,
        "go" => Language::Go,
        _ => return None,
    })
}

/// A pluggable structural parser for one language.
///
/// Implementors are intentionally tiny — the grammar plus a declaration
/// query. This is the Phase 2 plugin surface; out-of-tree languages can
/// implement this trait and be passed to [`crate::parse_declarations`].
pub trait LanguageParser {
    /// Which language this parser handles.
    fn language(&self) -> Language;

    /// The tree-sitter grammar.
    fn ts_language(&self) -> Result<tree_sitter::Language>;

    /// A tree-sitter query. Each match binds `@decl` and `@name`.
    fn declaration_query(&self) -> &'static str;
}

const RUST_QUERY: &str = r#"
[
  (function_item name: (identifier) @name) @decl
  (struct_item name: (type_identifier) @name) @decl
  (enum_item name: (type_identifier) @name) @decl
  (union_item name: (type_identifier) @name) @decl
  (trait_item name: (type_identifier) @name) @decl
  (mod_item name: (identifier) @name) @decl
  (type_item name: (type_identifier) @name) @decl
  (const_item name: (identifier) @name) @decl
  (static_item name: (identifier) @name) @decl
  (macro_definition name: (identifier) @name) @decl
  (impl_item type: (type_identifier) @name) @decl
]
"#;

const PYTHON_QUERY: &str = r#"
[
  (function_definition name: (identifier) @name) @decl
  (class_definition name: (identifier) @name) @decl
]
"#;

const TS_QUERY: &str = r#"
[
  (function_declaration name: (identifier) @name) @decl
  (class_declaration name: (type_identifier) @name) @decl
  (method_definition name: (property_identifier) @name) @decl
  (interface_declaration name: (type_identifier) @name) @decl
  (enum_declaration name: (identifier) @name) @decl
  (type_alias_declaration name: (type_identifier) @name) @decl
]
"#;

const GO_QUERY: &str = r#"
[
  (function_declaration name: (identifier) @name) @decl
  (method_declaration name: (field_identifier) @name) @decl
  (type_declaration (type_spec name: (type_identifier) @name)) @decl
]
"#;

struct RustParser;
struct PythonParser;
struct TsParser;
struct GoParser;

impl LanguageParser for RustParser {
    fn language(&self) -> Language {
        Language::Rust
    }
    fn ts_language(&self) -> Result<tree_sitter::Language> {
        Ok(tree_sitter_rust::LANGUAGE.into())
    }
    fn declaration_query(&self) -> &'static str {
        RUST_QUERY
    }
}

impl LanguageParser for PythonParser {
    fn language(&self) -> Language {
        Language::Python
    }
    fn ts_language(&self) -> Result<tree_sitter::Language> {
        Ok(tree_sitter_python::LANGUAGE.into())
    }
    fn declaration_query(&self) -> &'static str {
        PYTHON_QUERY
    }
}

impl LanguageParser for TsParser {
    fn language(&self) -> Language {
        Language::TypeScript
    }
    fn ts_language(&self) -> Result<tree_sitter::Language> {
        Ok(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
    }
    fn declaration_query(&self) -> &'static str {
        TS_QUERY
    }
}

impl LanguageParser for GoParser {
    fn language(&self) -> Language {
        Language::Go
    }
    fn ts_language(&self) -> Result<tree_sitter::Language> {
        Ok(tree_sitter_go::LANGUAGE.into())
    }
    fn declaration_query(&self) -> &'static str {
        GO_QUERY
    }
}

/// The built-in parser for a language, or [`Error::UnsupportedLanguage`].
pub fn parser_for(lang: Language) -> Result<Box<dyn LanguageParser>> {
    Ok(match lang {
        Language::Rust => Box::new(RustParser),
        Language::Python => Box::new(PythonParser),
        Language::TypeScript => Box::new(TsParser),
        Language::Go => Box::new(GoParser),
    })
}

/// Convenience: detect from a path and return its parser, if any.
pub fn parser_for_path(path: &str) -> Result<Box<dyn LanguageParser>> {
    let lang = detect_language(path).ok_or(Error::UnsupportedLanguage)?;
    parser_for(lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_extension() {
        assert_eq!(detect_language("src/a.rs"), Some(Language::Rust));
        assert_eq!(detect_language("m.py"), Some(Language::Python));
        assert_eq!(detect_language("c.tsx"), Some(Language::TypeScript));
        assert_eq!(detect_language("s.go"), Some(Language::Go));
        assert_eq!(detect_language("README.md"), None);
        assert_eq!(detect_language("Makefile"), None);
    }
}
