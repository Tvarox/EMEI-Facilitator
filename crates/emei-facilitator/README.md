# EMEI Facilitator

Backend service for the EMEI protocol — an on-chain invoicing and automated payment collection system built on Mantle Sepolia. Handles invoice lifecycle management, mandate-based auto-collection, Merkle receipt anchoring, reputation scoring, and real-time event indexing via webhook-driven architecture.

**Stack**: Rust (axum, tokio, alloy, sqlx) · PostgreSQL (Neon) · Redis (Cloud) · Alchemy Webhooks

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           EMEI Facilitator                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────────────────┐  │
│  │  HTTP Server │    │   Services   │    │        Data Layer            │  │
│  │  (axum)      │    │  (tokio)     │    │                              │  │
│  │              │    │              │    │  ┌────────┐  ┌───────────┐   │  │
│  │  /invoice    │    │  collector   │    │  │Postgres│  │   Redis   │   │  │
│  │  /mandate    │    │  scanner     │    │  │ (Neon) │  │  (Cloud)  │   │  │
│  │  /webhook    │───▶│  batcher     │    │  └────────┘  └───────────┘   │  │
│  │  /public/*   │    │  webhook_wkr │    │                              │  │
│  │  /health     │    │  indexer     │    └──────────────────────────────┘  │
│  └──────────────┘    └──────────────┘                                      │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
         ▲                     │
         │                     ▼
    Alchemy Webhook       Mantle Sepolia RPC
    (Address Activity)    (alloy provider)
```

---

## Event Processing Pipeline

The system uses a **webhook-first** architecture. Services submit transactions and move on immediately — confirmation and side-effects are handled asynchronously when the webhook confirms the tx landed on-chain.

```
┌──────────────┐     ┌───────────┐     ┌──────────────┐     ┌──────────────┐
│   Alchemy    │────▶│  POST     │────▶│    Redis     │────▶│   Webhook    │
│   Webhook    │     │ /emei/    │     │    Queue     │     │   Worker     │
│  (on-chain   │     │ webhook   │     │ (FIFO list)  │     │              │
│   confirm)   │     └───────────┘     └──────────────┘     └──────┬───────┘
└──────────────┘       returns 200                                  │
                       immediately                                  ▼
                                                          ┌─────────────────┐
                                                          │  • Upsert event │
                                                          │    as confirmed │
                                                          │  • Queue receipt│
                                                          │    hash (if Paid│
                                                          │  • giveFeedback │
                                                          │    (reputation) │
                                                          └─────────────────┘
```

### Why webhook-based?

1. **Non-blocking**: The webhook endpoint pushes raw JSON to Redis and returns `200` in <1ms. No parsing, no DB writes in the hot path.
2. **Reliable**: Redis list acts as a durable buffer. If the worker crashes, payloads remain in the queue.
3. **Decoupled**: Services (collector, scanner) submit txs and insert `pending` events. The webhook worker upgrades them to `confirmed` when Alchemy reports the tx is mined.
4. **Side-effects after confirmation only**: Receipt hashing and reputation feedback only trigger after on-chain confirmation — never optimistically.

---

## Background Services

Five services run as independent tokio tasks, spawned at startup:

| Service | Interval | Purpose |
|---------|----------|---------|
| `webhook_worker` | continuous (blocking pop) | Processes Alchemy webhook payloads from Redis queue |
| `auto_collector` | 10s | Scans chain for PRESENTED invoices with matching mandates, submits `collect()` |
| `overdue_scanner` | 60s | Marks invoices past due as overdue, applies reputation penalty |
| `receipt_batcher` | 30s | Drains pending receipt hashes, builds Merkle tree, posts root on-chain |
| `event_indexer` | one-shot | Backfills DB from chain state on first boot (if DB is empty), then sleeps |

### Service Startup Staggering

To avoid RPC rate-limit spikes on cold boot:
- `collector` waits 10s
- `scanner` waits 20s
- `indexer` waits 10s
- `batcher` starts immediately (queries chain for latest batch number)
- `webhook_worker` starts immediately (blocking pop on Redis)

---

## Invoice Lifecycle (End-to-End)

```
Agent A (issuer)                    Facilitator                         Chain
      │                                  │                                │
      │  POST /emei/invoice              │                                │
      │─────────────────────────────────▶│  send_user(createInvoice)      │
      │                                  │───────────────────────────────▶│
      │  ◀─── { tx_hash }               │                                │
      │                                  │  insert_event(pending)         │
      │                                  │                                │
      │                                  │                                │
      │  POST /emei/present              │                                │
      │─────────────────────────────────▶│  send_user(presentInvoice)     │
      │                                  │───────────────────────────────▶│
      │  ◀─── { tx_hash }               │                                │
      │                                  │                                │
      │                                  │                                │
      │         [auto_collector tick]    │                                │
      │                                  │  getInvoice() → status=1       │
      │                                  │  getMandatesByPayer()           │
      │                                  │  validate mandate rules         │
      │                                  │  send_hot(collect)             │
      │                                  │───────────────────────────────▶│
      │                                  │  insert_event(pending)         │
      │                                  │                                │
      │                                  │                                │
      │         [Alchemy webhook fires] │                                │
      │                                  │◀──── InvoicePaid log           │
      │                                  │  upsert_confirmed_event        │
      │                                  │  insert_pending_receipt        │
      │                                  │  send_hot(giveFeedback)        │
      │                                  │───────────────────────────────▶│
      │                                  │                                │
      │                                  │                                │
      │         [receipt_batcher tick]   │                                │
      │                                  │  drain_pending_receipts        │
      │                                  │  build Merkle tree             │
      │                                  │  send_hot(postMerkleRoot)      │
      │                                  │───────────────────────────────▶│
```

---

## Mandate Validation Rules

The `auto_collector` checks these conditions before calling `collect()`:

1. Invoice status == PRESENTED (1)
2. Invoice collection mode == mandate (0)
3. Mandate status == Active (0)
4. Current time within `[validFrom, validUntil]`
5. `remainingCap >= invoice.amount`
6. Invoice issuer is in `approvedCounterparties`
7. At least one invoice line-item category matches `approvedCategories` (or categories list is empty = no restriction)

---

## Overdue Detection

The `overdue_scanner` marks invoices as overdue based on payment terms:

| Term Type | Due Calculation |
|-----------|----------------|
| `due_on_receipt` (0) | `presentedAt + 300s` (5 min grace for demo) |
| `net_n_days` (1) | `presentedAt + (netDays × 86400)` |

When overdue:
1. Submits `markOverdue(invoiceId)` on-chain
2. Waits 8s for tx to land
3. Submits `giveFeedback(payer, invoiceId, 0)` — zero amount signals negative reputation

---

## API Reference

### Authenticated Endpoints (require EIP-191 signed request)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/emei/invoice` | Create a new invoice |
| POST | `/emei/present` | Present an invoice to payer |
| POST | `/emei/pay` | Pay an invoice directly |
| POST | `/emei/collect` | Collect via mandate (manual trigger) |
| POST | `/emei/mandate` | Create a spending mandate |
| DELETE | `/emei/mandate/:id` | Revoke a mandate |
| POST | `/emei/register` | Register identity (ERC-8004) |
| POST | `/emei/withdraw` | Withdraw from settlement vault |

### Query Endpoints (read-only)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/emei/invoice/:id` | Get invoice details from chain |
| GET | `/emei/statement` | Query events for a payer (paginated) |
| GET | `/emei/reputation/:address` | Get reputation score |
| GET | `/emei/balance/:address` | Get vault balance |
| GET | `/emei/verify/:id` | Verify receipt Merkle proof |
| GET | `/emei/paylink/:id` | Get pay-link data for an invoice |

### Public Dashboard Endpoints (no auth)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/emei/public/stats` | Aggregated protocol stats |
| GET | `/emei/public/events` | Recent events (paginated) |
| GET | `/emei/public/agents` | Known agents with balances/reputation |
| GET | `/emei/public/mandates` | Active mandates with spend data |

### Infrastructure

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check (RPC, DB, queue status) |
| POST | `/emei/webhook` | Alchemy webhook receiver |

---

## Database Schema (PostgreSQL)

```sql
-- Core event store (idempotent via tx_hash + log_index)
CREATE TABLE events (
    id          BIGSERIAL PRIMARY KEY,
    event_type  TEXT NOT NULL,          -- InvoiceCreated|Presented|Paid|Overdue|MandateCreated|...
    block_number BIGINT NOT NULL,
    tx_hash     TEXT NOT NULL,
    log_index   INTEGER NOT NULL,
    timestamp   BIGINT NOT NULL,
    invoice_id  BIGINT,
    payer       TEXT,
    issuer      TEXT,
    amount      TEXT,                   -- wei string
    params      TEXT DEFAULT '{}',      -- JSON metadata
    status      TEXT DEFAULT 'confirmed', -- pending | confirmed
    UNIQUE(tx_hash, log_index)
);

-- Receipt queue for Merkle batching
CREATE TABLE pending_receipts (
    id           BIGSERIAL PRIMARY KEY,
    receipt_hash BYTEA NOT NULL,        -- keccak256(invoiceId)
    invoice_id   BIGINT,
    created_at   BIGINT NOT NULL
);

-- Transaction tracking for confirmation monitoring
CREATE TABLE pending_txs (
    id           BIGSERIAL PRIMARY KEY,
    tx_hash      TEXT NOT NULL UNIQUE,
    sender       TEXT NOT NULL,
    nonce        BIGINT NOT NULL,
    submitted_at BIGINT NOT NULL,
    confirmed_at BIGINT,
    status       TEXT DEFAULT 'pending' -- pending | confirmed
);

-- Key-value store for indexer state
CREATE TABLE indexer_state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

---

## Redis Usage

| Key | Type | Purpose |
|-----|------|---------|
| `emei:queue:webhook` | List (FIFO) | Raw Alchemy webhook payloads awaiting processing |
| `emei:queue:receipts` | List | Receipt hashes (32 bytes each) for batching |
| `emei:nonce:<address>` | String (u64) | Atomic nonce counter per address |
| `emei:cache:*` | String + TTL | Cached responses (stats, etc.) |

The webhook worker uses `BRPOP` with a 5s timeout for efficient blocking consumption.

---

## Nonce Management (Redis-Backed)

All hot wallet transactions use **Redis-based atomic nonce management** via `redis_client.rs`. This ensures correctness across restarts and enables multi-instance deployments.

**How it works:**

1. On first `send_hot` call, the chain client queries the on-chain nonce via `eth_getTransactionCount`
2. Redis key `emei:nonce:<address>` is initialized via `SETNX` (only sets if key doesn't exist)
3. Each subsequent call atomically increments via `INCR` — no race conditions
4. On `nonce too low` errors, the system re-syncs from chain and resets the Redis key
5. On other tx failures, the nonce is decremented (`DECR`) so it can be reused next cycle

**Redis nonce keys:**

| Key | Type | Description |
|-----|------|-------------|
| `emei:nonce:0x<address>` | String (u64) | Next nonce to use for this address |

**Recovery behavior:**
- Restart: Redis retains the nonce. If it drifts, the first `nonce too low` error triggers a re-sync from chain.
- Redis flush: Next `send_hot` call re-initializes from chain nonce via `SETNX`.
- Multi-instance: `INCR` is atomic — two instances will never claim the same nonce.

---

## Configuration

All configuration is loaded from environment variables at startup:

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `EMEI_RPC_URL` | yes | — | Mantle Sepolia RPC endpoint |
| `EMEI_HOT_WALLET_KEY` | yes | — | Private key for background service txs |
| `EMEI_INVOICE_ADDRESS` | yes | — | EMEIInvoice contract address |
| `EMEI_MANDATE_ADDRESS` | yes | — | EMEIMandate contract address |
| `EMEI_SETTLEMENT_ADDRESS` | yes | — | EMEISettlement contract address |
| `EMEI_RECEIPT_ADDRESS` | yes | — | EMEIReceipt contract address |
| `EMEI_BAY8004_ADDRESS` | yes | — | Bay8004 reputation contract |
| `EMEI_ERC8004_ADDRESS` | yes | — | MockERC8004 identity registry |
| `DATABASE_URL` | yes | — | PostgreSQL connection string |
| `REDIS_URL` | yes | — | Redis connection string |
| `EMEI_BATCH_INTERVAL` | no | `30` | Seconds between receipt batch cycles |
| `EMEI_COLLECT_INTERVAL` | no | `10` | Seconds between collection scans |
| `EMEI_OVERDUE_INTERVAL` | no | `60` | Seconds between overdue scans |
| `DEMO_AGENTS` | no | — | Comma-separated `label:address` pairs for dashboard |

---

## Deployment (Render + Docker)

The service deploys as a single container running the Rust facilitator + TypeScript demo bots via `Dockerfile.combined`.

### Container Layout

```
/usr/local/bin/emei-server      ← Rust binary (HTTP + background services)
/opt/signal-bot/dist/           ← Compiled TypeScript bots
/entrypoint.sh                  ← Starts facilitator, waits for health, launches bots
```

### Render Configuration

| Setting | Value |
|---------|-------|
| Docker build path | `./Dockerfile.combined` |
| Health check path | `/health` |
| Port | `8080` |
| Plan | Free tier (single instance) |

### Entrypoint Flow

1. Start `emei-server` in background
2. Poll `/health` until ready (max 30s)
3. Start `signal-bot`, `compute-bot`, `analytics-bot` (if respective `*_PK` env vars are set)
4. Wait on facilitator PID (container exits if facilitator dies)

---

## Alchemy Webhook Setup

### Dashboard Configuration

1. Go to [Alchemy Dashboard → Webhooks](https://dashboard.alchemy.com/webhooks)
2. Create new webhook:
   - **Type**: Address Activity
   - **Chain**: Mantle Sepolia (5003)
   - **URL**: `https://<your-render-url>/emei/webhook`
3. Add monitored addresses:
   ```
   0xC35f709255D7199394655F16008e8d1A3AD80005  (EMEIInvoice)
   0xF48C3bd4FE046629A9c12A39693f39c297893bD8  (EMEIMandate)
   0xE61B57D84fb55E2601ab47B83c367612E348d409  (Bay8004)
   0x558a20766d5998765B056597b8b78fe1914f3969  (EMEIReceipt)
   ```

### Webhook Payload Structure (Alchemy Address Activity)

```json
{
  "webhookId": "wh_xxx",
  "type": "ADDRESS_ACTIVITY",
  "event": {
    "network": "MANTLE_SEPOLIA",
    "activity": [
      {
        "hash": "0xabc...",
        "blockNum": "0x1a2b",
        "log": {
          "topics": ["0x<event_sig>", "0x<indexed_1>", "0x<indexed_2>"],
          "data": "0x<non_indexed_data>",
          "logIndex": "0x0",
          "blockNumber": "0x1a2b",
          "transactionHash": "0xabc..."
        }
      }
    ]
  }
}
```

### Event Signature Parsing

The webhook worker identifies events by topic count and data length:

| Topics | Data Size | Event Type |
|--------|-----------|------------|
| 4 (sig + invoiceId + issuer + payer) | 32 bytes (amount) | `InvoiceCreated` |
| 3 (sig + invoiceId + payer) | ≥64 bytes | `InvoicePaid` |
| 3 (sig + invoiceId + payer) | 32 bytes | `InvoicePresented` |
| 3 (sig + invoiceId + payer) | 0 bytes | `InvoiceOverdue` |

### Fallback (No Webhook)

If Alchemy doesn't support Mantle Sepolia webhooks:
- The `event_indexer` backfills on empty DB
- The `auto_collector` still scans chain state directly
- The `overdue_scanner` still reads chain state
- Events will be `pending` in DB (never upgraded to `confirmed`)
- System remains functional, just without real-time confirmation tracking

---

## Smart Contracts

| Contract | Address | Role |
|----------|---------|------|
| EMEIInvoice | `0xC35f709255D7199394655F16008e8d1A3AD80005` | Invoice CRUD + lifecycle |
| EMEIMandate | `0xF48C3bd4FE046629A9c12A39693f39c297893bD8` | Spending mandates |
| EMEISettlement | `0xfdCb7bA077069A7Da44711Ee6bdB49174AFA4dD0` | Vault + settlement |
| EMEIReceipt | `0x558a20766d5998765B056597b8b78fe1914f3969` | Merkle receipt anchoring |
| Bay8004 | `0xE61B57D84fb55E2601ab47B83c367612E348d409` | Reputation scoring |
| MockERC8004 | `0x4B560970423B08632bC2Aa31D0a70e29e66Fca37` | Identity registry |

Chain: **Mantle Sepolia** (chain ID 5003)

---

## Local Development

```bash
# Prerequisites: Rust 1.75+, PostgreSQL, Redis

# Clone and enter
cd x402-rs/crates/emei-facilitator

# Copy env
cp .env.example .env
# Edit .env with your credentials

# Run
cargo run --bin emei-server

# Health check
curl http://localhost:8080/health
```

---

## Project Structure

```
src/
├── bin/server.rs          # Entry point: config → state → router → serve
├── lib.rs                 # Public API: emei_router(), start_services()
├── config.rs              # Environment variable loading + validation
├── state.rs               # AppState: chain + db + redis + queues + config
├── chain.rs               # Alloy-based chain client (call, send_hot, send_user)
├── redis_client.rs        # Redis: queues, nonce (atomic INCR), cache
├── merkle.rs              # Merkle tree construction for receipt batching
├── signing.rs             # EIP-191 signature verification for auth
├── error.rs               # Error types (Validation, Database, Chain, etc.)
├── contracts/             # ABI bindings (alloy sol! macros)
│   ├── invoice.rs
│   ├── mandate.rs
│   ├── settlement.rs
│   ├── receipt.rs
│   └── bay8004.rs
├── routes/
│   ├── mod.rs             # Route tree definition
│   ├── invoice.rs         # create, present, pay, collect
│   ├── mandate.rs         # create, revoke
│   ├── query.rs           # get_invoice, statement, reputation, balance
│   ├── receipt.rs         # Merkle proof verification
│   ├── identity.rs        # ERC-8004 registration
│   ├── withdraw.rs        # Vault withdrawal
│   ├── paylink.rs         # Pay-link generation
│   ├── public.rs          # Dashboard: stats, events, agents, mandates
│   ├── health.rs          # Health check
│   └── webhook.rs         # Alchemy webhook receiver → Redis
├── services/
│   ├── mod.rs             # start_services() spawns all tasks
│   ├── webhook_worker.rs  # Redis consumer → parse → confirm → side-effects
│   ├── collector.rs       # Auto-collect via mandates
│   ├── scanner.rs         # Overdue detection + reputation penalty
│   ├── batcher.rs         # Merkle receipt batching
│   └── indexer.rs         # One-time backfill on empty DB
├── db/
│   ├── mod.rs             # StatementStore (sqlx PgPool wrapper)
│   └── schema.rs          # DDL for all tables
└── types/                 # Shared request/response types
```

---

## Observability

Structured logging via `tracing` with service-level context:

```
emei_facilitator=info     # Default log level
tower_http=info           # HTTP request/response logging
```

Override with `RUST_LOG` environment variable:
```bash
RUST_LOG=emei_facilitator=debug,tower_http=debug
```

Key log events to monitor:
- `webhook_worker: events confirmed` — webhook processing successful
- `auto_collector: collection submitted` — mandate-based collection fired
- `overdue_scanner: invoice marked overdue` — overdue enforcement
- `receipt_batcher: batch posted` — Merkle root anchored
- `indexer: backfill complete` — initial sync done

---

## Security Notes

- **Hot wallet isolation**: Background services use a dedicated hot wallet key. User transactions are signed client-side and relayed via `send_user`.
- **No webhook signature validation** (testnet): Production should validate Alchemy's HMAC signature header.
- **No contract changes**: Contracts are in audit prep — all new logic is off-chain only.
- **Credential rotation**: If credentials are exposed, rotate `EMEI_HOT_WALLET_KEY`, `DATABASE_URL`, and `REDIS_URL` immediately.
