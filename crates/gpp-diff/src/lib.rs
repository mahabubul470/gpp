//! `gpp-diff` — diff engine.
//!
//! Two layers:
//!
//! * **Line-based** ([`stat`], [`unified`]) — the Phase 1 fallback, used for
//!   binary files and languages with no registered parser.
//! * **Semantic** ([`semantic`], [`SemanticDiff`]) — Phase 2. A tree-sitter
//!   parse extracts top-level declarations (functions, types, etc.), each
//!   fingerprinted so that added / removed / modified / renamed / moved
//!   declarations can be reported structurally instead of as raw lines.
//!
//! Languages plug in through the [`LanguageParser`] trait; Rust, Python,
//! TypeScript and Go ship built in. See `docs/ROADMAP.md` (Phase 2).
#![forbid(unsafe_code)]

mod error;
mod lang;
mod line;
mod parser;
mod semantic;

pub use error::{Error, Result};
pub use lang::{Language, LanguageParser, detect_language, parser_for, parser_for_path};
pub use line::{FileStat, LineOp, LineOpKind, excerpt, line_ops, stat, unified};
pub use parser::{Declaration, parse_declarations};
pub use semantic::{ChangeOp, FileSemanticDiff, Move, detect_moves, render, semantic};
