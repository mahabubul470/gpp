# CLI Specification — gpp (git++)

## Global Flags

```
--repo <path>       Override repository path (default: search upward for .gpp/)
--config <path>     Override config file
--color <auto|always|never>
--verbose, -v       Increase verbosity (repeat for more: -vv, -vvv)
--quiet, -q         Suppress non-error output
--json              Output as JSON (for scripting/agent consumption)
--help, -h          Show help
--version           Show version
```

## Core Commands

### gpp init

Initialize a new gpp repository.

```
gpp init [path] [flags]

Flags:
  --graphex           Enable Graphex knowledge graph layer
  --no-timeline       Disable continuous timeline capture
  --encryption        Enable full-repo encryption (not just Graphex)
  --git-bridge <url>  Initialize with Git bridge to existing repo
  --from-git <path>   Import from existing Git repository
  --template <name>   Use a project template (e.g., "fintech", "saas", "library")

Examples:
  gpp init
  gpp init ./my-project --graphex
  gpp init --from-git ../old-repo --graphex
  gpp init --git-bridge git@github.com:acme/webapp.git
```

### gpp status

Show working directory status, active timeline, and pending changes.

```
gpp status [flags]

Flags:
  --short, -s         One-line summary
  --timeline          Show recent timeline entries
  --agents            Show active agent sessions
  --cost              Show cost summary for current session

Output:
  On branch: main
  Timeline: active (1,247 entries, 3.2 MB)
  Last capture: 12 seconds ago (3 files)
  Unpromoted changes: 47 timeline entries since cs:a3f9b2
  Active agents: claude-code (trust: 94.2, session: 14m)
  Policy violations: 0
  Cost this session: $2.14 (8,420 tokens)
```

### gpp timeline

View and manage the continuous timeline.

```
gpp timeline [subcommand] [flags]

Subcommands:
  (default)           Show recent timeline entries
  watch               Live-stream timeline entries as they happen
  search              Search timeline by file, author, or content
  prune               Remove old timeline entries per retention policy
  export              Export timeline range to file

Flags:
  --since <time>      Show entries since time (e.g., "1h", "2026-05-16", "today")
  --until <time>      Show entries until time
  --author <id>       Filter by author
  --file <pattern>    Filter by file path glob
  --limit, -n <N>     Show last N entries (default: 20)
  --stat              Show file change statistics

Examples:
  gpp timeline                           # Last 20 entries
  gpp timeline --since 1h --author agent:claude-code
  gpp timeline watch                     # Live stream
  gpp timeline search --file "orders/**"
  gpp timeline prune --older-than 60d    # Prune entries older than 60 days
```

### gpp promote

Promote timeline entries to a curated changeset.

```
gpp promote [flags]

Flags:
  --from <time|id>    Start of timeline range
  --to <time|id>      End of timeline range (default: now)
  --message, -m <msg> Changeset description
  --intent <type>     Intent type: feature|bugfix|refactor|docs|dependency
  --task <ref>        Link to task/issue (e.g., "JIRA-1234")
  --interactive, -i   Interactively select which timeline entries to include
  --auto-summarize    Use AI to generate changeset message from timeline
  --sign              Cryptographically sign the changeset

Examples:
  gpp promote --from 14:00 --to 14:30 -m "Fixed orders rounding"
  gpp promote --interactive
  gpp promote --auto-summarize --intent bugfix --task PROJ-2847
```

### gpp log

View changeset history.

```
gpp log [flags]

Flags:
  --oneline           One line per changeset
  --graph             Show branch/merge graph
  --semantic          Show semantic change summaries
  --author <id>       Filter by author
  --agent             Show only agent-authored changesets
  --human             Show only human-authored changesets
  --since <time>
  --until <time>
  --file <path>       Show changesets affecting file
  --intent <type>     Filter by intent type
  -n <N>              Show last N changesets

Examples:
  gpp log --oneline --graph
  gpp log --agent --since "last week" --semantic
  gpp log --file "src/orders/" --intent bugfix
```

### gpp diff

Show changes with semantic awareness.

```
gpp diff [target] [flags]

Targets:
  (default)           Working directory vs last changeset
  <changeset>         Show diff for a specific changeset
  <cs1>..<cs2>        Diff between two changesets
  --timeline <id>     Diff at a specific timeline entry

Flags:
  --semantic          Show semantic operations (default for supported languages)
  --line              Force traditional line-based diff
  --stat              Show only statistics
  --files             Show only file names
  --language <lang>   Force language parser

Examples:
  gpp diff
  gpp diff cs:a3f9b2
  gpp diff cs:a3f9b2..cs:7e4d1c --semantic
  gpp diff --stat
```

### gpp branch

Manage branches and agent exploration branches.

```
gpp branch [subcommand] [name] [flags]

Subcommands:
  (default)           List branches
  create <name>       Create a new branch
  delete <name>       Delete a branch
  switch <name>       Switch to a branch
  explore <name>      Create an exploration branch (agent sandbox)

Flags:
  --agent <id>        Filter by agent (for exploration branches)
  --all, -a           Show all branches including explorations
  --merged            Show only merged branches
  --remote            Show remote branches

Examples:
  gpp branch
  gpp branch create feature/new-auth-flow
  gpp branch explore orders-bugfix   # Creates explorations/orders-bugfix
  gpp branch --agent claude-code               # Show all exploration branches by this agent
```

### gpp merge

Merge branches with AI-assisted conflict resolution.

```
gpp merge <branch> [flags]

Flags:
  --no-ai             Disable AI conflict resolution
  --strategy <s>      Merge strategy: semantic|ours|theirs|manual
  --dry-run           Show what would be merged without merging
  --squash            Squash into single changeset
  --accept-exploration <name>  Accept an exploration branch result

Examples:
  gpp merge feature/auth-flow
  gpp merge --accept-exploration orders-bugfix
  gpp merge feature/api-v2 --strategy semantic
```

## Graphex Commands

### gpp graphex

Knowledge graph management.

```
gpp graphex <subcommand> [flags]

Subcommands:
  status              Show graph statistics
  query <pattern>     Query the knowledge graph
  add                 Add a node
  link                Create an edge between nodes
  edit <node>         Edit a node
  remove <node>       Remove a node (soft delete)
  show <node>         Display full node details
  import <file>       Import nodes from YAML/JSON file
  export              Export graph to file
  visualize           Generate a visual representation
  federation          Manage cross-project federation
  audit               Show access log

Query Syntax:
  "node -> relation -> *"           Follow edges from node
  "* -> relation -> node"           Find what points to node
  "node -> * -> *"                  All edges from node
  "node -> relation -> * --depth N" Multi-hop traversal

Flags (for query):
  --type <type>       Filter by node type
  --tier <tier>       Filter by access tier
  --since <time>      Filter by creation/update time
  --depth <N>         Traversal depth (default: 1)
  --format <fmt>      Output format: text|json|mermaid|dot

Flags (for add):
  --type <type>       Node type (service|module|concept|convention|etc.)
  --tier <tier>       Access tier (default from config)
  --description, -d   Node description
  --properties, -p    Key=value properties (repeatable)

Examples:
  gpp graphex status
  gpp graphex query "orders-service -> depends-on -> *"
  gpp graphex query "* -> implements-policy -> pci-dss" --format mermaid
  gpp graphex add --type service --name "rate-limiter" -d "Token bucket rate limiter for API"
  gpp graphex link rate-limiter --relation depends-on --to redis-cache
  gpp graphex show orders-service
  gpp graphex visualize --output graph.svg
  gpp graphex audit --since "last week" --accessor agent:claude-code
  gpp graphex federation add --project project-b --subgraph "org-conventions"
```

## Trust Commands

### gpp trust

Agent trust management.

```
gpp trust <subcommand> [flags]

Subcommands:
  show                Show all agent trust scores
  history <agent>     Show trust score history for an agent
  policy              View/edit trust policies
  override <agent>    Manually override an agent's status
  reset <agent>       Reset an agent's trust score to default

Flags (for show):
  --agent <id>        Show specific agent
  --sort <field>      Sort by: score|name|last-active|changesets
  --above <score>     Filter agents above score
  --below <score>     Filter agents below score

Flags (for override):
  --status <s>        Set status: auto-merge|review-required|sandboxed|blocked
  --reason <msg>      Reason for override (logged)
  --duration <time>   Override duration (e.g., "7d", "permanent")

Examples:
  gpp trust show
  gpp trust show --agent claude-code
  gpp trust history claude-code --since "last month"
  gpp trust override copilot --status sandboxed --reason "Too many regressions in orders module"
  gpp trust policy                     # View current policy
  gpp trust policy --set auto_merge_min=95 --module "orders/**"
```

## Policy Commands

### gpp policy

Compliance policy management.

```
gpp policy <subcommand> [flags]

Subcommands:
  list                List active policies
  show <name>         Show policy details
  add <file>          Add a policy from file
  remove <name>       Remove a policy
  check               Run all policies against current state
  validate <file>     Validate a policy file syntax
  templates           List available policy templates

Flags (for check):
  --changeset <cs>    Check a specific changeset
  --file <path>       Check specific files
  --severity <s>      Filter by severity: block|warn|audit

Examples:
  gpp policy list
  gpp policy add policies/soc2.policy
  gpp policy check
  gpp policy check --changeset cs:a3f9b2
  gpp policy templates                 # Show built-in templates
  gpp policy add --template pci-dss    # Install from template
```

## Cost Commands

### gpp cost

Token and compute cost analytics.

```
gpp cost [flags]

Flags:
  --this-week         Show costs for current week
  --last-week         Show costs for last week
  --this-month        Show costs for current month
  --since <time>
  --until <time>
  --agent <id>        Filter by agent
  --module <pattern>  Filter by file path pattern
  --efficiency        Show cost per survived line of code
  --breakdown         Detailed breakdown by model/agent
  --budget            Show budget status
  --budget-alert <$>  Set weekly budget alert threshold
  --report <cs>       Report token/compute usage for a changeset
                      (HEAD, a short id, or a full hash). Accumulates.
  --model <id>        Model id for --report (e.g. claude-opus-4-8)
  --input <n>         Input (prompt) tokens for --report
  --output <n>        Output (completion) tokens for --report
  --cached <n>        Cached/prompt-cache tokens for --report
  --cost-micro <n>    Cost in micro-dollars for --report (1 = $0.000001)
  --duration-ms <n>   Wall-clock duration in ms for --report

Examples:
  gpp cost --this-week
  gpp cost --module "orders/**" --last-month
  gpp cost --agent claude-code --efficiency
  gpp cost --breakdown --this-month
  gpp cost --budget
  gpp cost --budget-alert 100.00
  gpp cost --report HEAD --model claude-opus-4-8 --input 1500 --output 300 --cost-micro 22000
```

An AI agent reports its own usage with `--report` after promoting (or via the
`report_cost` MCP tool / the SDK's `AgentSession::report_cost`). Until it does,
a changeset's cost is recorded as zero. See the MCP tutorial for the full agent
loop.

## Sync Commands

### gpp sync

Peer-to-peer synchronization.

```
gpp sync [subcommand] [flags]

Subcommands:
  (default)           Sync with all configured peers
  add <peer>          Add a sync peer
  remove <peer>       Remove a sync peer
  status              Show sync status with all peers
  push <peer>         Push to specific peer
  pull <peer>         Pull from specific peer

Flags:
  --force             Force sync even with conflicts
  --dry-run           Show what would sync
  --include-graphex   Include Graphex graph updates
  --exclude-timeline  Don't sync timeline (only history)

Peer format: <host>:<port> or <name> (from config)

Examples:
  gpp sync
  gpp sync add peer1.office.local:9473
  gpp sync status
  gpp sync push production-server
  gpp sync pull --include-graphex
```

## Replay Commands

### gpp replay

Reproduce agent sessions.

```
gpp replay <changeset> [flags]

Flags:
  --model <model>     Override model for replay
  --diff              Compare replay result with original
  --dry-run           Show what would be replayed without executing
  --output <path>     Save replay result to directory
  --env <key=val>     Override environment variables

Examples:
  gpp replay cs:a3f9b2
  gpp replay cs:a3f9b2 --model claude-opus-4-6 --diff
  gpp replay cs:a3f9b2 --dry-run
```

## Anomaly Commands

### gpp anomaly

View and manage anomaly alerts.

```
gpp anomaly [subcommand] [flags]

Subcommands:
  (default)           Show unresolved anomalies
  history             Show all anomalies
  resolve <id>        Mark an anomaly as resolved
  rules               List active detection rules
  configure <rule>    Configure a detection rule

Flags:
  --severity <s>      Filter: info|warning|review|block
  --agent <id>        Filter by agent
  --since <time>

Examples:
  gpp anomaly
  gpp anomaly history --since "last week"
  gpp anomaly resolve 42 --reason "Expected behavior during refactor"
  gpp anomaly rules
  gpp anomaly configure burst-activity --threshold 30 --window 5m
```

## Git Bridge Commands

### gpp git-import / gpp git-export / gpp git-bridge

```
gpp git-import <path> [flags]
  --branch <name>     Import specific branch only
  --since <time>      Import commits after date
  --shallow <N>       Import last N commits only

gpp git-export [flags]
  --to <path>         Export to Git repo at path
  --branch <name>     Export specific branch
  --push              Push after export
  --remote <name>     Git remote to push to

gpp git-bridge [flags]
  --watch             Continuous bidirectional sync
  --direction <d>     gpp-to-git | git-to-gpp | bidirectional
  --interval <sec>    Sync interval in seconds (default: 30)

Examples:
  gpp git-import ../legacy-repo
  gpp git-export --to ../git-mirror --push
  gpp git-bridge --watch --direction bidirectional
```

## Utility Commands

### gpp audit

Generate compliance audit reports.

```
gpp audit [flags]

Flags:
  --module <pattern>  Scope to module
  --since <time>
  --until <time>
  --format <fmt>      text|json|pdf|html
  --export <path>     Save report to file
  --include-graphex   Include knowledge graph access log
  --include-cost      Include cost attribution data

Examples:
  gpp audit --module "orders/**" --since 2026-01-01 --export audit-q1.pdf
  gpp audit --include-graphex --include-cost --format html
```

### gpp mcp-server

Start the MCP server for AI tool integration.

```
gpp mcp-server [flags]

Flags:
  --port <N>          Port for HTTP transport (default: 9474)
  --stdio             Use stdio transport (for tool integration)
  --allowed-tools     Comma-separated list of enabled MCP tools
  --trust-tier <t>    Maximum trust tier for connected agents

Examples:
  gpp mcp-server --stdio                    # For Claude Code / Cursor integration
  gpp mcp-server --port 9474 --trust-tier agent-readable
```

### gpp config

View and edit configuration.

```
gpp config <subcommand> [key] [value]

Subcommands:
  get <key>           Get a config value
  set <key> <value>   Set a config value
  list                List all config
  edit                Open config in $EDITOR

Scope:
  --local             Repository config (.gpp/config.toml)
  --global            Global config (~/.config/gpp/config.toml)

Examples:
  gpp config get trust.auto_merge_min
  gpp config set timeline.retention_days 60 --local
  gpp config list --global
```

## Review Commands

### gpp review

Code review workflow.

```
gpp review <subcommand> [flags]

Subcommands:
  list                List reviews (default: pending)
  show <changeset>    Show changeset with semantic diff + context for review
  request <changeset> Request review for a changeset
  approve <changeset> Approve a changeset
  request-changes <changeset>  Request changes with comments
  reject <changeset>  Reject a changeset
  merge <changeset>   Merge an approved changeset into target branch
  comments <changeset> Show review comments/threads

Flags (for list):
  --status <s>        Filter: pending|approved|changes_requested|rejected|merged
  --author <id>       Filter by changeset author
  --reviewer <id>     Filter by assigned reviewer
  --mine              Show reviews assigned to me

Flags (for approve/request-changes/reject):
  --message, -m <msg> Review comment
  --file <path>       Comment on specific file
  --line <N>          Comment on specific line (requires --file)

Examples:
  gpp review list --mine
  gpp review show cs:a3f9b2
  gpp review approve cs:a3f9b2 -m "LGTM, orders logic is correct"
  gpp review request-changes cs:a3f9b2 -m "Need tests for edge case" --file orders/batch.rs --line 142
  gpp review reject cs:a3f9b2 -m "Wrong approach, see exploration/orders-fix-claude"
  gpp review merge cs:a3f9b2
```

## Remote Platform Commands

### gpp remote

Interact with GitHub, GitLab, Bitbucket, or other platforms.

```
gpp remote <subcommand> [flags]

Subcommands:
  setup               Configure remote platform connection
  status              Show remote sync status

  pr create           Create PR/MR on remote platform (auto-enriched with gpp metadata)
  pr list             List open PRs/MRs
  pr show <id>        Show PR details with local gpp context
  pr merge <id>       Merge PR on remote
  pr sync             Sync reviews/comments bidirectionally

  ci status           Show CI/CD status for current branch
  ci logs <run>       Fetch CI run logs

  issues link <id>    Link current changeset to a remote issue
  issues list         List issues from remote platform

Flags (for pr create):
  --title <title>     PR title (default: changeset message)
  --body <body>       Additional PR body text
  --draft             Create as draft PR
  --base <branch>     Target branch (default: main)
  --no-enrich         Skip adding gpp metadata to PR body
  --reviewers <ids>   Request specific reviewers

Flags (for pr sync):
  --direction <d>     local-to-remote | remote-to-local | bidirectional

Examples:
  gpp remote setup --platform github --repo acme/webapp
  gpp remote pr create --title "Fix orders rounding" --reviewers maintainer1,maintainer2
  gpp remote pr list
  gpp remote pr sync --direction bidirectional
  gpp remote ci status
  gpp remote issues link PROJ-2847
```

**Implemented today** (the rest of the above is the target surface):

```
  gpp remote setup --platform github --repository acme/webapp --token-env GITHUB_TOKEN
  gpp remote status
  gpp remote pr-create [--base main] [--head <branch>] [--title <t>]
  gpp remote push [--branch main]            # plain Git push, no platform API

  # Inbound sync (GitHub; reads the live API via $GITHUB_TOKEN):
  gpp remote ci [--git-ref <branch|sha>]     # combined CI status for a commit
  gpp remote reviews --pr <n>                # PR review state + approval gate
```

## Notification Commands

### gpp inbox

View and manage notifications.

```
gpp inbox [flags]

Flags:
  --unread            Show unread count only
  --type <type>       Filter by event type
  --since <time>
  --limit, -n <N>     Show last N notifications (default: 20)

Subcommands:
  (default)           Show notifications
  ack <id>            Acknowledge a notification
  ack --all           Acknowledge all notifications
  settings            View/edit notification preferences

Examples:
  gpp inbox
  gpp inbox --unread
  gpp inbox ack 42
  gpp inbox ack --all
  gpp inbox --type review.requested
  gpp inbox settings
```

### gpp notify

Manage notification integrations.

```
gpp notify <subcommand> [flags]

Subcommands:
  integrations        List configured notification backends
  add <backend>       Add a notification backend (slack|discord|email|webhook|jira|linear)
  remove <backend>    Remove a notification backend
  test <backend>      Send a test notification
  events              List subscribable event types

Examples:
  gpp notify integrations
  gpp notify add slack --webhook "https://hooks.slack.com/..." --channel "#webapp-dev"
  gpp notify add webhook --url "https://ci.example.com/hooks/gpp" --events changeset.promoted
  gpp notify add jira --base-url "https://example.atlassian.net" --project PROJ
  gpp notify test slack
  gpp notify events
```

## RBAC Commands

### gpp rbac

Human permission management.

```
gpp rbac <subcommand> [flags]

Subcommands:
  show                Show all role assignments
  assign <identity> <role>  Assign a role (owner|maintainer|contributor|reader)
  revoke <identity>   Remove role assignment
  protect <branch>    Set branch protection rules
  whoami              Show current user's role and permissions

Flags (for assign):
  --reason <msg>      Reason for role change (logged)
  --expires <time>    Role expiration (e.g., "30d", "2026-12-31")

Flags (for protect):
  --min-reviewers <N> Minimum reviewers to merge
  --require-human     Require at least one human reviewer
  --require-role <r>  Minimum role to merge (default: maintainer)
  --allow-agent-merge Allow auto-merge eligible agents to merge
  --require-policy    All policies must pass before merge

Examples:
  gpp rbac show
  gpp rbac whoami
  gpp rbac assign maintainer1@example.com maintainer --reason "Promoted to tech lead"
  gpp rbac assign auditor@example.com reader --expires 90d
  gpp rbac revoke dev3@example.com
  gpp rbac protect main --min-reviewers 2 --require-human --require-policy
  gpp rbac protect "release/*" --require-role owner --min-reviewers 2
```

## Relay Commands

### gpp relay

Manage relay node connections (client-side).

```
gpp relay <subcommand> [flags]

Subcommands:
  status              Show relay connection status
  add <address>       Add a relay node
  remove <name>       Remove a relay node
  push                Push to relay
  pull                Pull from relay

Examples:
  gpp relay add office-relay 192.168.1.50:9473
  gpp relay add cloud relay.example.com:9473
  gpp relay status
  gpp relay push
  gpp relay pull
```

### gpp-relay (Separate Binary)

Run a relay node.

```
gpp-relay [flags]

Flags:
  --port <N>          Listen port (default: 9473)
  --storage <path>    Object storage directory
  --max-repos <N>     Maximum repositories to host
  --auth-keys <path>  Authorized peer keys file
  --log-level <l>     debug|info|warn|error

Examples:
  gpp-relay --port 9473 --storage /data/gpp
  gpp-relay --port 9473 --storage /data/gpp --auth-keys /etc/gpp/authorized_keys
```

## Terminal UI Command

### gpp ui

Launch the interactive terminal UI.

```
gpp ui [flags]

Flags:
  --panel <name>      Open with specific panel focused: timeline|history|graphex|agents|reviews|anomalies|cost|inbox
  --layout <l>        Layout preset: default|minimal|review|monitoring
  --no-live           Disable live timeline updates

Examples:
  gpp ui
  gpp ui --panel reviews
  gpp ui --layout monitoring         # Focus on agents, anomalies, cost
  gpp ui --panel graphex             # Start in knowledge graph explorer
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid arguments / usage error |
| 3 | Policy violation (blocked) |
| 4 | Trust violation (agent blocked) |
| 5 | Sync conflict |
| 6 | Encryption error (missing key, wrong passphrase) |
| 7 | Permission denied (RBAC) |
| 8 | Remote platform API error |
| 10 | Not a gpp repository |
| 11 | Repository corrupted |
