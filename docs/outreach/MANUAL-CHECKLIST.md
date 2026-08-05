# Outreach — manual steps (need Mahabubul's account)

Everything automatable from the outreach handoff is done and committed.
These remaining steps need your GitHub account, or are judgment calls
about actually publishing. Ordered — earlier items unblock later ones.

## 1. Repo settings (github.com/mahabubul470/gpp → ⚙ About)

- [x] **Description / Website / Topics** — set 2026-07-12 via
      `gh repo edit` (description, homepage, and all 7 topics verified).
- [x] Confirm the deployed site renders as intended — verified 2026-08-06
      (title, hero, demo GIF, all sections render). (Pages deploy is
      green), and that a shared link unfurls with the new OG image —
      test in a Slack/Discord DM to yourself.

## 2. crates.io (do BEFORE posting anywhere — name reservation)

The whole `gpp-*` namespace is free on crates.io (checked 2026-07-12;
bare `gpp` is taken by an unrelated preprocessor crate — our binary
crate is `gpp-cli`, which still installs a `gpp` binary). The workspace
dry-run passes end to end; publishing is wired into release.yml and
activates once the token exists.

- [x] Log in at crates.io with GitHub, create an API token
      (Account Settings → API Tokens, scope: publish-new + publish-update).
- [x] Add it as a repo secret named `CARGO_REGISTRY_TOKEN`
      (Settings → Secrets and variables → Actions).
- [x] All 21 crates published to crates.io 2026-07-12; `cargo install
      gpp-cli` verified end to end from a clean root (installs, inits,
      promotes, bisects). Future releases publish via the resumable
      scripts/publish-crates.sh (rate-limit aware, idempotent).
- [x] README/site/docs install commands switched to
      `cargo install gpp-cli` (git variant documented as the
      development path).

## 3. First release (v0.1.0)

- [x] Tag and push: `git tag v0.1.0 && git push origin v0.1.0`
      — release.yml builds 4 targets (linux-x86_64, macos-arm64,
      macos-x86_64, windows-msvc) + pushes Docker images to ghcr.io,
      and creates the GitHub Release with generated notes.
      **Watch the first run** — the Windows and macos-x86_64 legs are
      new and unverified; if one fails, the others still upload
      (fail-fast is off).
- [x] Release notes intro added 2026-07-12 (wedge paragraph, install
      line, honest-scope note, demo links).

## 4. Homebrew tap

- [x] Create repo `mahabubul470/homebrew-tap`. Done 2026-08-06:
      <https://github.com/mahabubul470/homebrew-tap>.
- [x] Compute the tarball hash — done 2026-08-06:
      `aca859e00ca4bd8b5c7559137264bf2ec549cba5744db0666fb1cc2fa778197e`,
      already filled into `packaging/homebrew/gpp.rb` (unneeded cmake
      build dep dropped at the same time — nothing in Cargo.lock uses it).
- [x] Copy `packaging/homebrew/gpp.rb` into the tap as `Formula/gpp.rb`,
      push. Done 2026-08-06 (`Formula/gpp.rb` + README on `main`).
- [ ] Verify: `brew install mahabubul470/tap/gpp` on a Mac (or
      `brew install --build-from-source`). The install docs already
      reference this tap path.

## 5. Publishing the writing

- [ ] Blog post: `docs/outreach/blog-belief-bisect.md` — publish on the
      Pages site (or leave on GitHub and link the raw doc). If you give
      it a nicer URL, update the link inside
      `docs/outreach/social-drafts.md`.
- [ ] Show HN: title + text in `docs/outreach/social-drafts.md` §(a).
      Submit the **repo URL**, put the text as a first comment.
      Best window: weekday morning US time.
- [ ] r/rust: §(b) of the same file. Flair as "project"; engage on
      implementation questions (the post invites review of
      `crates/gpp-graphex/src/stale.rs` — expect comments on the
      rusqlite bundled-SQLite / "no C deps" tension; the honest answer
      is "no *hand-written* C, bundled SQLite is the one vetted
      exception").
- [ ] X/Twitter thread: §(c) outline; attach `site/assets/demo.gif` to
      the first tweet.

## 6. MCP directory listings

- [x] punkpeye/awesome-mcp-servers — PR opened 2026-08-06 (Version
      Control section, agent fast-track title):
      <https://github.com/punkpeye/awesome-mcp-servers/pull/11586>.
      Watch for maintainer feedback.
- [~] Official MCP Registry (registry.modelcontextprotocol.io) — the
      modelcontextprotocol/servers community list was *retired* in favor
      of this registry. Wired up 2026-08-06: `server.json` at repo root
      (name `io.github.mahabubul470/gpp`, cargo package `gpp-cli`),
      crate README with the visible `mcp-name:` ownership token,
      gpp-cli bumped to 0.1.1 (crates.io READMEs are per-version), and
      `.github/workflows/mcp-registry-publish.yml` (GitHub OIDC, no
      secrets). Sequence: publish-crates workflow (gpp-cli 0.1.1) →
      mcp-registry-publish workflow. Verify at
      <https://registry.modelcontextprotocol.io/> afterwards.

## 7. Nice-to-have follow-ups (no account needed, ask Claude)

- [ ] Re-record `scripts/demo.sh` after any CLI output change
      (`asciinema rec --window-size 100x32 --overwrite -c ./scripts/demo.sh site/assets/demo.cast`
      then `agg --font-size 16 --theme dracula site/assets/demo.cast site/assets/demo.gif`).
- [ ] Once the tap + release exist, add `brew install mahabubul470/tap/gpp`
      and binary-download instructions to README's Install section.
