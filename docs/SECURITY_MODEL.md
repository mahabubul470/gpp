# Security Model — gpp (git++)

## Threat Model

### Assets to Protect

1. **Source code** — The actual codebase
2. **Project knowledge** — Graphex graph (architecture, conventions, domain terms)
3. **Agent context** — What was projected to which agent (prevents knowledge leakage)
4. **Credentials & secrets** — API keys, tokens, passwords, private keys
5. **Compliance data** — Audit trails, policy results, provenance chains

### Threat Actors

| Actor | Capability | Goal |
|-------|-----------|------|
| Malicious agent | Can read projected context, propose changes | Exfiltrate code/knowledge, introduce backdoors |
| Compromised peer | Has sync access, can send/receive objects | Inject malicious code, steal knowledge graph |
| External attacker | Network access | Intercept sync traffic, compromise remote peers |
| Insider (rogue developer) | Full repo access | Bypass policies, tamper with audit trails |
| AI provider | Receives agent context | Collect proprietary code/knowledge from projections |

## Encryption Architecture

### At Rest

| Data | Encryption | Key |
|------|-----------|-----|
| Code blobs | Optional (repo-level flag) | Repo master key (age) |
| Graphex nodes | Always encrypted | Per-tier keys (age) |
| Timeline database | Optional | Repo master key |
| Trust database | Not encrypted | Local-only, no sensitive content |
| Config files | Not encrypted | May contain peer addresses (not secrets) |
| Object store | zstd compressed; encrypted if flag set | Repo master key |

### In Transit

All peer-to-peer communication uses Noise_XX protocol (same pattern as WireGuard):
- Forward secrecy (ephemeral Diffie-Hellman per session)
- Mutual authentication (static keys)
- AES-256-GCM for symmetric encryption after handshake
- No TLS/certificate dependency

### Key Management

```
Repository Master Key (age identity)
├── Generated at `gpp init`
├── Stored at .gpp/graphex/keys/master.age
├── Backed up by developer (recovery phrase or key file)
│
├── Tier Keys (derived or separately generated)
│   ├── public_tier_key           # Plaintext (no encryption needed)
│   ├── agent_readable_tier_key   # Encrypted with master key
│   ├── agent_restricted_tier_key # Encrypted with master key
│   └── human_only_tier_key       # Encrypted with master key + passphrase
│
└── Peer Sync Keys (Noise static keys)
    ├── Generated per peer relationship
    └── Exchanged out-of-band (key ceremony or config)
```

### Key Rotation

```bash
gpp keys rotate --tier agent-readable    # Rotate specific tier key
gpp keys rotate --all                    # Rotate all keys
gpp keys rotate --master                 # Rotate master key (re-encrypts everything)
```

Rotation re-encrypts all affected objects. The old key is kept in a key archive for decrypting historical snapshots.

## Agent Security

### Agent Sandboxing

Agents operate within constraints enforced by the trust engine and policy engine:

1. **Identity verification.** Every agent session starts with authentication. The agent declares its name, model, and session ID. The SDK signs this with a session key.

2. **Context projection limits.** Agents only receive graph content matching their trust tier. A sandboxed agent (score < 70) gets only `public` nodes. A trusted agent (score > 90) gets `public` + `agent-readable` nodes. No agent ever sees `human-only` or `agent-restricted` nodes unless running in a locally trusted environment.

3. **Write restrictions.** Agents cannot directly write to the object store, history, or graph. They can only propose changesets and graph updates through the SDK. All proposals are queued for human review (unless trust score permits auto-merge for specific scope).

4. **Exploration branches.** Agent work happens on isolated exploration branches. These branches don't affect main until explicitly accepted by a human.

5. **Rate limiting.** The anomaly detection layer monitors for unusual agent behavior (burst activity, unusual scope, etc.) and can pause or block an agent.

### Preventing Context Leakage

The core problem: when you send context to an external AI provider (Anthropic, OpenAI, etc.), you lose control of that data.

gpp mitigates this through:

1. **Minimal projection.** Only send the minimum context the agent needs. The Graphex projection engine computes the smallest relevant subgraph.

2. **Scrubbing.** Projections strip sensitive properties, credentials, and `agent-restricted` content before leaving the local machine.

3. **Context budget.** Each projection has a token budget. The projection engine prioritizes high-relevance nodes and truncates low-relevance ones.

4. **Audit trail.** Every projection is logged: what was sent, to which agent, when. If a leak is suspected, you can trace exactly what data was exposed.

5. **Local-only mode.** For maximum security, agents can run locally (e.g., local LLM via Ollama). In local mode, `agent-restricted` content can be included in projections since nothing leaves the machine.

```toml
# .gpp/config.toml
[graphex.projection]
max_tokens = 4000               # Max tokens per projection
scrub_patterns = [              # Regex patterns to scrub from projections
    '(?i)(password|secret|key|token)\s*[=:]\s*\S+',
    '\d{4}[\s-]?\d{4}[\s-]?\d{4}[\s-]?\d{4}',  # Card numbers
]
allow_restricted_local = true   # Allow agent-restricted in local-only mode
```

## Secrets Prevention

### Storage-Layer Blocking

Unlike Git hooks (which can be skipped), gpp's policy engine operates at the storage layer. A secret detected during timeline capture is flagged immediately. A secret in a changeset being promoted is blocked.

### Built-in Patterns

The default `secrets-scan` policy detects:
- API keys (generic patterns + provider-specific: AWS, GCP, GitHub, Stripe, etc.)
- Private keys (RSA, EC, DSA, Ed25519)
- Connection strings (database URLs with credentials)
- JWT tokens
- OAuth tokens
- Generic high-entropy strings in assignment context

### Custom Patterns

```toml
# .gpp/policies/custom-secrets.policy
[[rules]]
type = "pattern"
pattern = 'INTERNAL_[A-Z]+_KEY\s*=\s*["\'][^"\']{16,}'
message = "Internal API key detected"
severity = "block"
```

### Secret Remediation

If a secret is accidentally captured in the timeline:

```bash
gpp timeline scrub --pattern 'sk_live_[a-zA-Z0-9]{24}'  # Remove matching content from timeline
gpp gc --purge                                            # Garbage collect unreferenced objects
```

This rewrites timeline entries to replace the secret with `[REDACTED]` and removes the original blob from the object store.

## Audit & Compliance

### Provenance Chain

Every changeset has a full provenance chain:

```
Human prompt → Agent session (model, config, context projected)
  → Timeline entries (raw file changes)
  → Changeset (curated, reviewed)
  → Policy results (which policies passed/failed)
  → Deployment (linked via metadata)
```

### Audit Log

```bash
gpp audit --since 2026-01-01 --module "orders/**" --format json
```

Produces:
- All changesets touching the module
- Author (human or agent) for each changeset
- Intent (what was the goal)
- Policy results (what was checked)
- Graphex access log (what context was projected to agents)
- Cost records (what did it cost)
- Trust events (any score changes)
- Anomaly events (any flags raised)

### Tamper Detection

The changeset DAG uses cryptographic hashes (BLAKE3). Any modification to a historical changeset changes its hash, which breaks the chain. Optional Ed25519 signatures on changesets provide non-repudiation.

```bash
gpp verify --changeset cs:a3f9b2   # Verify hash chain integrity
gpp verify --all                    # Verify entire history
```

## Peer Authentication

### Trust on First Use (TOFU)

When adding a new sync peer, the static public keys are exchanged out-of-band:

```bash
# On peer A:
gpp sync keygen          # Generate Noise static key pair
gpp sync show-key        # Display public key (share with peer B)

# On peer B:
gpp sync add peer-a 192.168.1.50:9473 --key <peer-a-public-key>
```

After initial key exchange, peers authenticate automatically on every connection.

### Peer Permissions

```toml
# .gpp/config.toml
[sync.peers.office-backup]
address = "backup.example.com:9473"
key = "age1..."
permissions = ["objects", "history", "policies"]  # No graphex sync
```

Permissions control what data flows to each peer:
- `objects` — Code blobs and trees
- `history` — Changeset DAG and refs
- `graphex` — Knowledge graph (encrypted)
- `policies` — Policy rules
- `all` — Everything

## Human RBAC Security

### Role Enforcement

RBAC roles (owner/maintainer/contributor/reader) are enforced at two levels:

1. **Local CLI enforcement.** The CLI checks the current user's role before executing privileged operations. A contributor cannot merge to a protected branch. A reader cannot promote changesets. Role checks happen before any state mutation.

2. **Sync protocol enforcement.** The relay/peer validates the sender's role before accepting pushed objects. A contributor can push changesets but not role changes. A reader's pushes are rejected entirely. Role validation uses the sender's cryptographic identity (Noise static key) mapped to their role assignment.

### Role Change Security

Role changes are themselves versioned as changesets with special metadata:
- Only owners can assign/revoke roles
- Role change changesets require the owner's cryptographic signature
- Role history is append-only and auditable
- A minimum of one owner must always exist (the system prevents removing the last owner)

### Separation of Agent and Human Permissions

Agent trust and human RBAC are deliberately separate systems:
- Agents have trust scores, not roles. An agent never has "owner" or "maintainer" privileges.
- Humans have roles, not trust scores. A human maintainer is always trusted for review operations regardless of any computed metric.
- The only intersection: a maintainer can override an agent's trust score, and trust policies define which human roles can override which agent states.

## Remote Platform Security

### GitHub/GitLab Token Management

Platform API tokens are never stored in gpp's config files. They're read from environment variables:

```toml
[remote]
api_token_env = "GITHUB_TOKEN"    # Read from $GITHUB_TOKEN at runtime
```

The token is used only for API calls and never written to disk, objects, or the timeline.

### PR Enrichment Data Flow

When gpp enriches a PR description with metadata (intent, semantic diff, agent info, cost), the following data flows to the remote platform:
- Changeset message and intent description (human-readable text, not encrypted)
- Semantic diff summary (structural operations, not raw code diffs)
- Agent name and trust score (no model prompts or context projections)
- Cost summary (aggregate tokens and dollars, not token-level detail)
- Policy results (pass/fail, not full policy rule definitions)

What does NOT flow to the remote platform:
- Graphex node content (encrypted, stays local)
- Context projections (what was shown to the agent, stays local)
- Timeline entries (local safety net, stays local)
- Trust event history (local analytics, stays local)
- Full policy rules (local enforcement, stays local)

### Review Sync Security

When bidirectional review sync is enabled, review decisions and comments flow between gpp and the remote platform. This means platform reviewers see gpp-generated context and gpp sees platform comments. Both directions are authenticated via the platform API token. If review sync is disabled (`mirror_reviews = false`), no review data crosses the boundary.

## Relay Node Security

### Relay Trust Model

The relay is explicitly NOT a trusted authority. It cannot:
- Read Graphex content (encrypted, relay doesn't have tier keys)
- Modify objects (content-addressed, any modification changes the hash)
- Override RBAC roles (role changes require owner signature)
- Bypass policy enforcement (policies are checked by the receiving peer, not the relay)

The relay CAN:
- Store and forward encrypted objects
- Track which peers have synced which objects
- Reject pushes from unauthorized peers (based on Noise key authentication)
- Apply rate limits and storage quotas

### Relay Authentication

Every peer connecting to a relay must authenticate with its Noise static key. The relay maintains an authorized keys list:

```bash
gpp-relay --auth-keys /etc/gpp/authorized_keys
```

Unauthorized connections are rejected at the Noise handshake level — no objects are exchanged.

### Relay Compromise Scenario

If a relay is compromised, the attacker gains:
- Encrypted blobs they cannot read (code is zstd+optionally encrypted, Graphex is always encrypted)
- Object hashes (which reveal nothing about content)
- Sync metadata (who synced when, which peers exist)

The attacker cannot:
- Read source code (if repo encryption is enabled)
- Read Graphex knowledge (always encrypted)
- Inject objects that peers would accept (hash verification on receive)
- Impersonate peers (Noise authentication)

Mitigation:
```bash
gpp sync remove compromised-relay
gpp relay add new-relay <new-address>
# No key rotation needed unless the relay also had tier keys (it shouldn't)
```

## Incident Response

### Compromised Agent

```bash
gpp trust override <agent-id> --status blocked --reason "Compromised"
gpp timeline scrub --author agent:<agent-id> --since <compromise-time>
gpp graphex reject --author agent:<agent-id> --since <compromise-time>
```

### Compromised Peer

```bash
gpp sync remove <peer-name>
gpp keys rotate --all                    # Rotate all keys
gpp verify --all                         # Verify history integrity
```

### Leaked Secret

```bash
gpp timeline scrub --pattern '<secret-pattern>'
gpp gc --purge
# Also: rotate the actual secret in your infrastructure
```
