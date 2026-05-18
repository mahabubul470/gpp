# Tutorial: Using gpp with GitHub

```bash
gpp remote setup --platform github --repository acme/webapp --token-env GITHUB_TOKEN
export GITHUB_TOKEN=ghp_…
gpp promote -m "Add retry queue"
gpp remote pr-create --base main          # PR enriched with gpp metadata
```

The PR description carries intent, semantic-change summary, agent, policy
and trust — while GitHub only receives clean Git commits.

Or use the `gh` extension end-to-end:

```bash
gh extension install ./extensions/gh-gpp
gh gpp promote -m "Add retry queue"
gh gpp trust          # post trust scores as a PR comment
gh gpp sync           # import the GitHub default branch back into gpp
```

For platforms without an API, `gpp remote push` exports clean Git and
`git push`es it.
