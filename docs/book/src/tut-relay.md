# Tutorial: Setting up a relay node

A relay is just an always-on peer. It stores **encrypted** objects and
forwards them; it never has tier keys and cannot read your code or graph.

On the relay host:

```bash
gpp-relay --port 9473 --storage /data/gpp \
  --auth-keys /etc/gpp/authorized_keys
# health: curl http://relay:9474/health  → {"status":"ok","objects":N}
```

Or via Docker:

```bash
docker run -p 9473:9473 -p 9474:9474 -v gpp-data:/data ghcr.io/mahabubul470/gpp-relay
```

On each developer machine:

```bash
gpp relay add office relay.host:9473
gpp relay push office     # then teammates: gpp relay pull office
```

Divergent same-name branches are preserved as `name.fork.<peer>` — resolve
explicitly with `gpp merge name.fork.<peer>` (never a silent merge).
