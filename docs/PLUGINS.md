# Plugin surfaces

gpp has three extension points, all stable Rust traits / file formats —
no dynamic loading, so plugins are compiled in or shelled out to.

## 1. Language parsers (semantic diff)

Implement `gpp_diff::LanguageParser`:

```rust
pub trait LanguageParser {
    fn language(&self) -> Language;
    fn ts_language(&self) -> Result<tree_sitter::Language>;
    fn declaration_query(&self) -> &'static str;   // binds @decl and @name
}
```

A parser is just a tree-sitter grammar + a declaration query; the generic
extractor and the add/remove/modify/rename/move detector are language-
agnostic. Built in: Rust, Python, TypeScript, Go. Pass any `&dyn
LanguageParser` to `gpp_diff::parse_declarations`.

## 2. Policy templates

TOML `*.policy` files (see `policies/README.md`). Pattern rules (regex on
content) and changeset rules (author / files / review). Installed per-repo,
synced between peers.

## 3. Compliance report formatters

`gpp audit` emits a stable, line-oriented report (and `--json` where
supported); downstream formatters (HTML/PDF/SARIF) consume that — the CI
actions in `.github/actions/` are the reference consumers.

## Editor / tool integration

The `gpp` CLI (with `--json` on the data commands) is the single source of
truth. `extensions/vscode-gpp` and `extensions/neovim-gpp` are thin shells
over it; `gpp mcp-server --stdio` is the AI-agent surface.
