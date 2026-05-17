# gh-gpp

A [GitHub CLI](https://cli.github.com) extension that brings gpp intelligence
to your GitHub workflow.

## Install

```bash
gh extension install ./extensions/gh-gpp     # from a gpp checkout
# or, once published:
gh extension install gpp-vcs/gh-gpp
```

Requires `gpp` and `gh` on `PATH`, and `GITHUB_TOKEN` for PR creation.

## Commands

| Command | What it does |
|---|---|
| `gh gpp promote -m "msg"` | Promote a changeset, push to GitHub, open a PR enriched with intent / semantic diff / cost / policy / trust |
| `gh gpp review [ref]` | Show a changeset with semantic diff + gpp review context |
| `gh gpp trust` | Post agent trust scores as a PR comment |
| `gh gpp cost` | Post token/compute cost attribution as a PR comment |
| `gh gpp audit [--gist]` | Generate a cross-layer audit report (optionally as a gist) |
| `gh gpp sync` | Import the GitHub default branch into gpp |

## Notes

- This is a **Bash** `gh` extension (the `gh` extension convention accepts
  any executable named `gh-<name>`; a Go rewrite is optional and tracked in
  the roadmap).
- GitHub only ever sees clean Git commits. Graphex, timeline, trust, cost
  and policies stay local — `gh gpp` surfaces them *into* the PR as
  comments/description, never pushes them to the repo.
