# Sync Protocol — gpp (git++)

## Overview

gpp uses a CRDT-based, offline-first, peer-to-peer synchronization protocol. There is no central server requirement. Any gpp repository can sync with any other gpp repository directly.

## Design Goals

1. **Offline-first.** Work without network, sync when reconnected. All operations are local.
2. **No central authority.** Any peer is equal. No single point of failure.
3. **Convergent.** All peers that have seen the same set of operations will have the same state.
4. **Efficient.** Only transfer what the other peer doesn't have.
5. **Encrypted in transit.** All peer communication uses Noise protocol (like WireGuard).
6. **Zero-knowledge graph sync.** Graph structure can sync without decrypting node content.

## Data Synchronization Layers

| Layer | What Syncs | CRDT Type | Notes |
|-------|-----------|-----------|-------|
| Objects | Blobs, trees, changesets | Set (add-only) | Content-addressed, immutable |
| History | Changeset DAG | DAG CRDT | Parents define partial order |
| Refs | Branch pointers | LWW Register | Last-writer-wins per ref |
| Graphex | Graph nodes + edges | OR-Set | Add/remove with unique tags |
| Trust | Agent scores | Local only | **Never synced** — each peer computes independently |
| Timeline | File changes | Local only | **Never synced** — local safety net only |
| Policies | Policy rules | Set (add-only) | Shared policies sync; local overrides don't |

## Sync Handshake Protocol

### Step 1: Discovery

Peers are configured explicitly (no auto-discovery for security):

```toml
# .gpp/config.toml
[sync]
peers = [
    { name = "office", address = "192.168.1.50:9473" },
    { name = "backup", address = "backup.example.com:9473" },
]
```

### Step 2: Connect (Noise Protocol)

```
Initiator                              Responder
    │                                      │
    ├──── Noise_XX handshake ──────────────┤
    │     (ephemeral keys, then static)    │
    │                                      │
    ├──── Auth: repo ID + peer identity ───┤
    │                                      │
    ├──── Protocol version negotiation ────┤
    │                                      │
    ▼     Encrypted channel established    ▼
```

Transport: TCP + Noise_XX pattern. Both peers authenticate with their static keys. The repo ID ensures both sides are syncing the same repository.

### Step 3: State Exchange

Both peers exchange their "state vector" — a compact summary of what they have:

```rust
struct StateVector {
    repo_id: Hash,
    // Object store: bloom filter of known object hashes
    object_bloom: BloomFilter,      // False positive rate ~0.01%
    // History: set of changeset hashes (tips of all branches)
    branch_tips: BTreeMap<String, Hash>,
    // Graphex: vector clock of graph operations
    graph_vector_clock: VectorClock,
    // Policies: hash of active policy set
    policy_set_hash: Hash,
    // Timestamp of last sync with this peer
    last_sync: i64,
}
```

### Step 4: Delta Computation

Each peer computes what the other is missing:

```
Local state vector  ─┐
                     ├─→ Compute delta ─→ List of objects/operations to send
Remote state vector ─┘
```

For objects: use bloom filter to identify candidates, then send exact hash lists for the candidates to confirm what's actually missing.

For history: walk the changeset DAG from branch tips backward until reaching changesets the remote peer already has.

For Graphex: use vector clock comparison to identify unseen operations.

### Step 5: Transfer

```rust
enum SyncMessage {
    // Object transfer
    ObjectBatch { objects: Vec<(Hash, Vec<u8>)> },
    ObjectRequest { hashes: Vec<Hash> },

    // History transfer
    ChangesetBatch { changesets: Vec<Changeset> },
    RefUpdate { name: String, hash: Hash, timestamp: i64 },

    // Graphex transfer
    GraphOperationBatch { operations: Vec<GraphOperation> },

    // Policy transfer
    PolicySet { policies: Vec<PolicyRule> },

    // Control
    SyncComplete,
    Error { code: u32, message: String },
}
```

Objects are transferred in batches of up to 1MB. The receiving peer validates each object (hash check) before acknowledging.

### Step 6: Convergence

After transfer, both peers apply received operations:
- Objects: store in object store (idempotent — same hash = same content)
- History: add changesets to DAG, update refs using LWW
- Graphex: apply operations to OR-Set, resolve conflicts
- Policies: merge policy sets

## Conflict Resolution

### History Conflicts (Divergent Branches)

When two peers have different branch tips for the same branch name, gpp does NOT auto-merge. Instead:

```
Peer A: main → cs:abc123
Peer B: main → cs:def456

After sync, both peers see:
  main    → cs:abc123  (Peer A's version, by LWW)
  main@B  → cs:def456  (Peer B's version, preserved as fork)
```

The developer then explicitly merges:
```bash
gpp merge main@B   # Merge Peer B's divergent main into local main
```

### Graphex Conflicts

Graph operations use an OR-Set CRDT (Observed-Remove Set):
- **Add wins over remove** when concurrent (if one peer adds a node while another removes it, the node stays)
- **Property conflicts** use LWW (last-writer-wins based on timestamp)
- **Edge conflicts** are resolved by keeping both — humans review and prune

### Ref Conflicts

Branch refs (pointers to changesets) use Last-Writer-Wins Register:
- Each ref update carries a Lamport timestamp
- Higher timestamp wins
- Ties broken by peer ID (deterministic ordering)

## Bandwidth Optimization

### Object Deduplication

Content-addressed storage means identical files are never transferred twice, even across different changesets or branches.

### Thin Packs

For initial clone or large syncs, objects are packed into "thin packs" — delta-compressed bundles where similar objects are stored as deltas against a base. Similar to Git's pack files but using zstd dictionary compression for better ratios.

### Incremental Sync

After initial clone, syncs only transfer new objects since `last_sync`. The bloom filter exchange is O(n) in filter size, not in object count.

### Graph-Only Sync

For federated Graphex subgraphs, you can sync just the graph layer without syncing code:

```bash
gpp sync --graph-only --peer conventions-server
```

## Zero-Knowledge Graph Sync

When syncing Graphex with a peer, the following information is shared:
- Graph structure (which nodes exist, which edges connect them)
- Node metadata (type, name, access tier, timestamps)
- Encrypted node content blobs (the peer gets the ciphertext but can't read it without the right tier key)

The peer CANNOT read:
- Node descriptions or properties (encrypted)
- Convention text (encrypted)
- Glossary definitions (encrypted)

This means a backup server can hold a full copy of the graph for redundancy without being able to read the project's knowledge. Only peers with the appropriate tier keys can decrypt.

## Network Topology Examples

### Solo Developer

```
Laptop ←──sync──→ NAS (backup)
```

### Small Team

```
Dev A ←──sync──→ Dev B
  │                │
  └──sync──→ Office Server ←──sync──┘
```

### Organization with Federation

```
Project A ←──sync──→ Project A Backup
    │
    ├──federation──→ Shared Conventions ←──federation──┐
    │                                                   │
Project B ←──sync──→ Project B Backup                   │
    │                                                   │
    └──federation──→ Shared Conventions ────────────────┘
```

## Wire Protocol

### Message Framing

```
┌───────────────┐
│ Length: u32    │  Payload length (big-endian)
│ Type: u8      │  Message type enum
│ Payload: [u8] │  MessagePack-encoded body
│ MAC: [u8; 16] │  Noise protocol MAC
└───────────────┘
```

### Message Types

| Type | Code | Direction | Description |
|------|------|-----------|-------------|
| `Hello` | 0x01 | Both | Initial handshake |
| `StateVector` | 0x02 | Both | State vector exchange |
| `ObjectBatch` | 0x10 | Both | Batch of objects |
| `ObjectRequest` | 0x11 | Both | Request missing objects |
| `ChangesetBatch` | 0x20 | Both | Batch of changesets |
| `RefUpdate` | 0x21 | Both | Branch ref update |
| `GraphOpBatch` | 0x30 | Both | Batch of graph operations |
| `PolicySet` | 0x40 | Both | Policy set |
| `Ack` | 0xF0 | Both | Acknowledgment |
| `Error` | 0xFE | Both | Error message |
| `Done` | 0xFF | Both | Sync complete |

## Failure Handling

- **Connection lost mid-sync:** Resume from last acknowledged batch. State vector exchange is repeated but cheap.
- **Corrupt object received:** Reject (hash doesn't match), request retransmission.
- **Clock skew:** Lamport timestamps are logical, not wall-clock. No NTP dependency.
- **Peer permanently unavailable:** Other peers still work. No single point of failure.
- **Storage full:** Sync pauses with clear error. Resume when space available.

## Relay Node Protocol

The relay node (`gpp-relay`) is a specialized peer optimized for always-on availability.

### Relay Behavior

The relay differs from a regular peer in these ways:

1. **No working directory.** The relay doesn't check out files. It stores objects and forwards sync operations only.
2. **No timeline.** The relay never captures file changes (there are no files).
3. **No trust computation.** Trust scores are local; the relay doesn't compute them.
4. **No policy enforcement.** Policies are enforced by the receiving peer, not the relay.
5. **RBAC enforcement.** The relay DOES check RBAC roles — it rejects pushes from peers without the required role.
6. **Multi-repo support.** A single relay can host multiple repositories, each isolated.

### Relay Configuration

```toml
# /etc/gpp-relay/config.toml
[relay]
port = 9473
storage = "/data/gpp"
max_repos = 100
max_storage_gb = 500
log_level = "info"

[auth]
authorized_keys = "/etc/gpp-relay/authorized_keys"
allow_anonymous_read = false    # Public repos could enable this

[limits]
max_object_size_mb = 100
max_batch_size_mb = 50
rate_limit_per_peer = 1000     # Max sync operations per minute per peer
```

### Relay Sync Flow

```
Developer A                     Relay                      Developer B
    │                             │                             │
    ├── push changeset ──────────→│                             │
    │   (relay stores objects,    │                             │
    │    validates RBAC role)     │                             │
    │                             │                             │
    │                             │←── pull ────────────────────┤
    │                             │    (relay sends delta)      │
    │                             ├── objects + changesets ─────→│
    │                             │                             │
```

The relay is transparent — Developer B sees the same objects as if they synced directly with Developer A. The relay just adds persistence and availability.

## RBAC-Aware Sync

The sync protocol enforces RBAC roles at the protocol level.

### Role Checks During Sync

| Operation | Required Role | Enforcement Point |
|-----------|--------------|-------------------|
| Pull objects/history | Reader or above | Relay/peer |
| Push objects/history | Contributor or above | Relay/peer |
| Push ref updates (non-protected) | Contributor or above | Relay/peer |
| Push ref updates (protected branch) | Maintainer or above | Relay/peer |
| Push role changes | Owner | Relay/peer |
| Push policy changes | Maintainer or above | Relay/peer |
| Pull Graphex (encrypted) | Reader or above | Relay/peer (can't decrypt) |
| Push Graphex operations | Contributor or above | Relay/peer |

### Identity Resolution

The sync protocol resolves peer identity from their Noise static key:
1. Peer authenticates via Noise handshake (static key verified)
2. Relay/peer maps static key to identity (email/fingerprint)
3. Identity looked up in RBAC roles table
4. Role checked against required permission for the operation
5. If role insufficient, operation rejected with error code 7 (Permission denied)

## Review Sync

Reviews can sync between peers alongside changesets.

### What Syncs

| Data | Syncs? | Notes |
|------|--------|-------|
| Review objects | Yes | Status, decisions, policy requirements |
| Review comments | Yes | File/line-targeted comments |
| ConversationThreads | Yes | Full thread history |
| Remote PR links | No | Platform-specific, local to each peer |

### Review Conflict Resolution

If two reviewers approve on different peers before syncing:
- Both approvals are kept (additive)
- The review status resolves to the most advanced state (approved > pending)
- If conflicting decisions exist (one approve, one reject), status stays pending and both decisions are visible for the maintainer to resolve

## Port & Service

Default port: **9473** (TCP)
Service name: `gpp-sync`
Multicast discovery (LAN only, optional): `239.73.73.73:9473`
