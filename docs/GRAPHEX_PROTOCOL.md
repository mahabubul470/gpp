# Graphex Protocol — gpp (git++)

## Overview

Graphex is an encrypted, versioned, federated knowledge graph that lives inside a gpp repository. It is the "project brain" — a structured representation of architecture, conventions, domain knowledge, and decisions that AI agents query to understand the codebase without re-reading every file.

## Design Principles

1. **Local-first.** The graph lives on disk, not in a cloud service. Agents query it locally.
2. **Encrypted by default.** Every node is individually encrypted. Access is tiered.
3. **Versioned with code.** Graph mutations create timeline entries and appear in changesets.
4. **Agent-queryable.** Agents never see raw nodes — they get projected context filtered by trust tier.
5. **Federated.** Organizations can share subgraphs across projects.
6. **Human-approved.** Agent-proposed graph updates require human approval before persisting.

## Node Lifecycle

```
                  ┌──────────────┐
                  │   Proposed   │ ← Agent proposes via SDK, or auto-inferred from code
                  └──────┬───────┘
                         │ Human reviews
                         ▼
                  ┌──────────────┐
                  │   Active     │ ← Queryable, included in context projections
                  └──────┬───────┘
                         │ Contradicted or superseded
                         ▼
                  ┌──────────────┐
                  │  Deprecated  │ ← Still visible but flagged, not projected to agents
                  └──────┬───────┘
                         │ Explicit removal
                         ▼
                  ┌──────────────┐
                  │   Archived   │ ← Soft-deleted, recoverable, not visible
                  └──────────────┘
```

## Encryption Model

### Envelope Encryption

Each node is encrypted with its own symmetric key (AES-256-GCM). Those symmetric keys are themselves encrypted with the repository's master key (age identity).

```
Node content → AES-256-GCM(node_key) → Encrypted blob
Node key → age(master_key) → Encrypted key envelope
```

### Key Hierarchy

```
Master Key (age identity, held by repo owner)
  └── Tier Keys (one per access tier)
       ├── public_tier_key          → Not encrypted (plaintext readable)
       ├── agent_readable_tier_key  → Encrypted with master + agent tier key
       ├── agent_restricted_tier_key→ Encrypted with master key only
       └── human_only_tier_key      → Encrypted with master key + passphrase
```

### Access Tiers

| Tier | Who can read | When projected to agents | Storage |
|------|-------------|------------------------|---------|
| `public` | Anyone | Always | Plaintext |
| `agent-readable` | Humans + trusted agents | Projected as structured summary | Encrypted, agent-decryptable |
| `agent-restricted` | Humans + local-only agents | Only in trusted local runtime | Encrypted, master-key only |
| `human-only` | Humans with passphrase | Never | Encrypted, passphrase-gated |

### Context Projection

When an agent queries Graphex, the projection engine:

1. Receives the query and the agent's trust score
2. Resolves the subgraph matching the query
3. Filters nodes by access tier (agent's trust level determines max readable tier)
4. Decrypts readable nodes
5. Flattens into a structured text summary:
   - Node names and types
   - Relationship names (but not full node content for restricted nodes)
   - Convention descriptions
   - Glossary term definitions
6. Truncates to fit the agent's context budget
7. Logs the projection (nodes accessed, projection hash, agent ID, timestamp)
8. Returns the projected context

Projection format (returned to agent):

```
## Project Context: orders-service

### Architecture
- orders-service is a Service
- Depends on: currency-utils (internal), api-gateway (gRPC), postgresql (database)
- Owned by: orders-team
- Implements policies: pci-dss, soc2

### Conventions
- All monetary values stored as integers in cents (1 USD = 100 cents)
- Order batches processed in FIFO order
- External API calls always go through the gateway service, never direct
- Error codes follow ERR-XXXX format

### Glossary
- Order batch: Group of transactions processed together in a single batch cycle
- Idempotency key: Unique token attached to each request to make retries safe
- Tier 2 verification: Account verified with an ID document, allows higher transaction limits

### Recent Decisions
- 2026-03-15: Switched from float to integer arithmetic for orders calculations
- 2026-04-02: Added retry queue for failed upstream API calls (max 3 retries, exponential backoff)
```

## Graph Update Protocol

### Human-Initiated Updates

```
gpp graphex add --type convention --name "error-code-format" \
    -d "All error codes follow the format ERR-XXXX where XXXX is a 4-digit number"
```

Directly creates an Active node. No approval needed.

### Agent-Proposed Updates

Agents cannot directly mutate the graph. They propose updates that humans approve.

```rust
// Agent SDK
session.propose_graph_update(GraphUpdate::AddNode {
    name: "retry-queue",
    node_type: NodeType::Module,
    description: "Exponential backoff retry queue for failed upstream API calls",
    suggested_edges: vec![
        ("retry-queue", EdgeRelation::DependsOn, "api-gateway"),
        ("orders-service", EdgeRelation::DependsOn, "retry-queue"),
    ],
})?;
```

This creates a Proposed node. The developer sees:

```
gpp graphex pending

  Proposed by: agent:claude-code (trust: 94.2)
  Session: fix-orders-timeout

  + [module] retry-queue
    "Exponential backoff retry queue for failed upstream API calls"
    + retry-queue → depends-on → api-gateway
    + orders-service → depends-on → retry-queue

  Accept? [y/n/edit]
```

### Auto-Inferred Updates

When the semantic diff engine detects structural changes (new module, new dependency, removed service), it can propose graph updates automatically:

```
gpp detected: New file src/retry_queue/mod.rs with public API
  → Propose adding node "retry-queue" (type: module)?

gpp detected: New import in orders-service: use retry_queue::RetryQueue
  → Propose adding edge: orders-service → depends-on → retry-queue?
```

Auto-inference runs after each changeset promotion and produces Proposed nodes subject to human approval.

## Federation Protocol

### Concept

Organizations with multiple projects can share subgraphs. For example, Acme might have:

- `webapp` project with order-processing domain knowledge
- `project-b` project with analytics domain knowledge
- A shared `org-conventions` subgraph with company-wide coding standards

### Federation Setup

```toml
# webapp/.gpp/config.toml
[graphex.federation]
sources = [
    { project = "org-conventions", subgraph = "coding-standards", access = "read-only" },
    { project = "org-conventions", subgraph = "glossary", access = "read-only" },
]

publish = [
    { subgraph = "order-domain", nodes = ["orders-*", "billing-*", "auth-*"] },
]
```

### Sync Mechanics

Federated nodes carry a `FederatedFrom` source tag. They're read-only in the consuming project. Updates flow one-way: source project pushes updates, consuming projects pull.

Federation uses the same CRDT sync protocol as peer-to-peer repo sync, but scoped to graph operations on the federated subgraph.

### Access Control

The publishing project controls what gets federated:
- Node access tiers still apply — a `human-only` node is never federated
- The publisher defines which nodes are in the federated subgraph
- The consumer can't elevate access tiers — if a node is `agent-restricted` in the source, it's at least `agent-restricted` in the consumer

## Storage Format

### On-Disk Layout

```
.gpp/graphex/
├── graph.db          # SQLite adjacency index (see DATA_MODEL.md)
├── keys/
│   ├── master.age    # Master key (age encrypted)
│   ├── public.key    # Public tier key (plaintext)
│   ├── agent-readable.age
│   ├── agent-restricted.age
│   └── human-only.age
├── pending/          # Proposed updates awaiting approval
│   └── <hash>.proposal
└── federation/
    ├── sources.toml
    └── cache/        # Cached federated subgraphs
```

### Node Serialization

```
Node on disk = object store blob at .gpp/objects/<hash>
Blob content = wire_format(encrypted(zstd(msgpack(GraphNode))))
```

The SQLite index (`graph.db`) stores only metadata needed for queries — the actual node content is in the object store, encrypted.

## Query Language

### Syntax

```
<subject> -> <relation> -> <object>
```

Where:
- `<subject>` and `<object>` are node names, IDs, or `*` (wildcard)
- `<relation>` is an edge relation name or `*`
- `->` is directional; `<->` would be bidirectional (for bidirectional edges)

### Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `->` | Follow edge direction | `auth -> depends-on -> *` |
| `*` | Match any | `* -> owns -> orders-service` |
| `--depth N` | Multi-hop | `auth -> * -> * --depth 3` |
| `--type T` | Filter node type | `* -> * -> * --type service` |
| `--tier T` | Filter access tier | `* -> * -> * --tier public` |
| `--since T` | Filter by time | `* -> * -> * --since 2026-01-01` |
| `--confidence N` | Minimum confidence | `* -> * -> * --confidence 0.8` |

### Response Format (JSON)

```json
{
  "query": "orders-service -> depends-on -> *",
  "results": [
    {
      "path": ["orders-service", "depends-on", "currency-utils"],
      "nodes": {
        "orders-service": {
          "type": "service",
          "description": "Core orders processing engine",
          "confidence": 1.0
        },
        "currency-utils": {
          "type": "module",
          "description": "Currency conversion and integer arithmetic utilities",
          "confidence": 0.95
        }
      },
      "edge": {
        "relation": "depends-on",
        "confidence": 1.0,
        "created_at": "2026-03-15T10:00:00Z"
      }
    }
  ],
  "total_results": 5,
  "truncated": false
}
```

## MCP Integration

gpp's MCP server exposes tools across all layers. Agents connect once and get access to everything their trust tier permits.

### Graphex Tools

| Tool | Description | Input | Output |
|------|-------------|-------|--------|
| `graphex_query` | Query the knowledge graph | Query string, depth, filters | Projected context (text or JSON) |
| `graphex_status` | Get graph statistics | — | Node/edge counts, last update |
| `graphex_propose_node` | Propose adding a node | Node type, name, description, properties | Proposal ID |
| `graphex_propose_edge` | Propose adding an edge | From, to, relation | Proposal ID |
| `graphex_propose_update` | Propose updating a node | Node ID, updated fields | Proposal ID |
| `graphex_glossary` | Look up domain terms | Term name or partial match | Definition and context |
| `graphex_conventions` | List applicable conventions | File path or module name | Convention descriptions |

### Core Workflow Tools

| Tool | Description | Input | Output |
|------|-------------|-------|--------|
| `timeline_status` | Get recent timeline entries | Count, filters | Timeline entries list |
| `propose_changeset` | Promote timeline entries to changeset | Time range, message, intent | Changeset ID |
| `begin_exploration` | Create an exploration branch | Branch name, description | Branch ref |
| `end_exploration` | Finalize exploration with result | Accept or abandon | Result status |

### Governance Tools

| Tool | Description | Input | Output |
|------|-------------|-------|--------|
| `trust_status` | Check current agent trust score | — | Score, status, permissions |
| `policy_check` | Validate changes against policies | File paths or changeset | Policy results |
| `cost_estimate` | Estimate token cost for context projection | Query scope | Estimated tokens and cost |
| `cost_report` | Report actual token consumption | Input/output/cached tokens | Cost record ID |

### Review Tools (Tier 3 agents only)

| Tool | Description | Input | Output |
|------|-------------|-------|--------|
| `review_list` | List reviews assigned to or by agent | Filters | Review summaries |
| `review_comment` | Add a review comment | Changeset, file, line, comment | Comment ID |
