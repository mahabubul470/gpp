# Tutorial: Setting up Graphex

```bash
gpp init --graphex .                       # provisions the key hierarchy
gpp keys show
gpp graphex add --type service --name orders-service \
  -d "Core orders processing engine" --tier public
gpp graphex add --type convention --name money-format \
  -d "All monetary values stored as integer cents" --tier public
gpp graphex add --type glossary --name idempotency-key \
  -d "Token making retries safe" --tier human-only
gpp graphex link orders-service --relation depends-on --to currency-utils
gpp graphex query "orders-service -> depends-on -> *"
gpp graphex project --tier agent-readable     # what an agent would receive
```

The `human-only` glossary node is never decrypted for an agent-readable
projection. Rotate keys with `gpp keys rotate` (re-encrypts every node).
