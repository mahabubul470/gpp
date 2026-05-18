# Tutorial: Migrating from Git to gpp

```bash
cd my-existing-git-repo
gpp init .
gpp git-import .            # import all local branches' history
gpp log --oneline           # your Git history, now as gpp changesets
gpp diff HEAD               # semantic diff for supported languages
```

Keep using Git in parallel — export back any time:

```bash
gpp git-export .            # gpp history → Git commits (idempotent)
```

`gpp git-bridge <path> --watch` keeps the two in sync continuously.
Nothing about Graphex/timeline/trust/cost leaks into Git.
