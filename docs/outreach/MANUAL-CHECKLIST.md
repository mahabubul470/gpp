# Outreach — manual steps (need Mahabubul's account)

Everything automatable from the outreach handoff is done and committed.
These remaining steps need your GitHub account, or are judgment calls
about actually publishing. Ordered — earlier items unblock later ones.

## 1. Repo settings (github.com/mahabubul470/gpp → ⚙ About)

- [x] **Description / Website / Topics** — set 2026-07-12 via
      `gh repo edit` (description, homepage, and all 7 topics verified).
- [ ] Confirm the deployed site renders as intended (Pages deploy is
      green), and that a shared link unfurls with the new OG image —
      test in a Slack/Discord DM to yourself.

## 2. crates.io (do BEFORE posting anywhere — name reservation)

The whole `gpp-*` namespace is free on crates.io (checked 2026-07-12;
bare `gpp` is taken by an unrelated preprocessor crate — our binary
crate is `gpp-cli`, which still installs a `gpp` binary). The workspace
dry-run passes end to end; publishing is wired into release.yml and
activates once the token exists.

- [ ] Log in at crates.io with GitHub, create an API token
      (Account Settings → API Tokens, scope: publish-new + publish-update).
- [ ] Add it as a repo secret named `CARGO_REGISTRY_TOKEN`
      (Settings → Secrets and variables → Actions).
- [x] All 21 crates published to crates.io 2026-07-12; `cargo install
      gpp-cli` verified end to end from a clean root (installs, inits,
      promotes, bisects). Future releases publish via the resumable
      scripts/publish-crates.sh (rate-limit aware, idempotent).
- [x] README/site/docs install commands switched to
      `cargo install gpp-cli` (git variant documented as the
      development path).

## 3. First release (v0.1.0)

- [ ] Tag and push: `git tag v0.1.0 && git push origin v0.1.0`
      — release.yml builds 4 targets (linux-x86_64, macos-arm64,
      macos-x86_64, windows-msvc) + pushes Docker images to ghcr.io,
      and creates the GitHub Release with generated notes.
      **Watch the first run** — the Windows and macos-x86_64 legs are
      new and unverified; if one fails, the others still upload
      (fail-fast is off).
- [ ] After the release exists, edit the release notes intro: one
      paragraph on the wedge + link to the demo GIF and
      demos/belief-bisect. (Generated notes only list commits.)

## 4. Homebrew tap

- [ ] Create repo `mahabubul470/homebrew-tap`.
- [ ] Compute the tarball hash:
      `curl -sL https://github.com/mahabubul470/gpp/archive/refs/tags/v0.1.0.tar.gz | sha256sum`
- [ ] Copy `packaging/homebrew/gpp.rb` into the tap as `Formula/gpp.rb`
      with the real `sha256`, push.
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

- [ ] Submit `docs/outreach/mcp-listing.md` text as PRs to:
      - modelcontextprotocol/servers (community list)
      - punkpeye/awesome-mcp-servers (or the currently-canonical
        awesome list)
      Each wants: name, one-liner, config snippet — all in the listing
      doc verbatim.

## 7. Nice-to-have follow-ups (no account needed, ask Claude)

- [ ] Re-record `scripts/demo.sh` after any CLI output change
      (`asciinema rec --window-size 100x32 --overwrite -c ./scripts/demo.sh site/assets/demo.cast`
      then `agg --font-size 16 --theme dracula site/assets/demo.cast site/assets/demo.gif`).
- [ ] Once the tap + release exist, add `brew install mahabubul470/tap/gpp`
      and binary-download instructions to README's Install section.
