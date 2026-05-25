# EMEI Facilitator

**Production-grade blockchain transaction orchestrator for the EMEI invoice protocol on Mantle Sepolia.**

The EMEI Facilitator is a high-throughput Rust backend that manages the full lifecycle of on-chain invoices — from creation through settlement — with durable transaction queuing, atomic nonce management, webhook-driven state confirmation, and a multi-wallet pool for concurrent transaction submission.

---

## Table of Contents

- [Architecture Overview](#architecture-overview)
- [Invoice Lifecycle](#invoice-lifecycle)
- [Transaction Queue & Wallet Pool](#transaction-queue--wallet-pool)
- [Nonce Management](#nonce-management)
- [Webhook Pipeline](#webhook-pipeline)
- [Background Workers](#background-workers)
- [State Machine](#state-machine)
- [Scaling: 1 Agent vs 1000 Agents](#scaling-1-agent-vs-1000-agents)
- [Database Schema](#database-schema)
- [API Reference](#api-reference)
- [Configuration](#configuration)
- [Running](#running)

---

## Architecture Overview

```mermaid
graph TB
    subgraph "API Layer"
        API[Axum HTTP Server :8080]
    end

    subgraph "Queue Layer"
        REDIS[(Redis)]
        PGQUEUE[(PostgreSQL tx_queue)]
    end

    subgraph "Worker Pool"
        WH[Webhook Worker]
        TX1[TX Sender wallet_0]
        TX2[TX Sender wallet_1]
        TXN[TX Sender wallet_N]
        REAPER[TX Reaper]
    end

    subgraph "Background Services"
        BATCHER[Receipt Batcher]
        COLLECTOR[Auto-Collector]
        SCANNER[Overdue Scanner]
        INDEXER[Event Indexer]
    end

    subgraph "External"
        CHAIN[Mantle Sepolia RPC]
        ALCHEMY[Alchemy Webhooks]
    end

    ALCHEMY -->|POST /emei/webhook| API
    API -->|LPUSH| REDIS
    WH -->|BRPOP| REDIS
    WH -->|upsert confirmed events| PGQUEUE

    API -->|enqueue_tx| PGQUEUE
    COLLECTOR -->|enqueue_tx| PGQUEUE
    SCANNER -->|enqueue_tx| PGQUEUE
    BATCHER -->|enqueue_tx| PGQUEUE

    TX1 -->|claim SKIP LOCKED| PGQUEUE
    TX2 -->|claim SKIP LOCKED| PGQUEUE
    TXN -->|claim SKIP LOCKED| PGQUEUE

    TX1 -->|send_transaction| CHAIN
    TX2 -->|send_transaction| CHAIN
    TXN -->|send_transaction| CHAIN

    REAPER -->|reclaim stuck jobs| PGQUEUE
    INDEXER -->|backfill on startup| CHAIN
```

---

## Invoice Lifecycle

```mermaid
sequenceDiagram
    participant Issuer
    participant API as EMEI Facilitator
    participant Chain as Mantle Sepolia
    participant Alchemy
    participant Redis
    participant WebhookWorker as Webhook Worker
    participant DB as PostgreSQL

    Note over Issuer,DB: === CREATE ===
    Issuer->>API: POST /emei/invoice (X-Private-Key)
    API->>API: Validate request + encode ABI calldata
    API->>Chain: send_transaction (user signer, auto-nonce)
    Chain-->>API: tx_hash
    API->>DB: INSERT event (status=pending)
    API-->>Issuer: 201 { tx_hash }

    Note over Issuer,DB: === PRESENT ===
    Issuer->>API: POST /emei/present { invoice_id }
    API->>Chain: send_transaction (presentCall)
    Chain-->>API: tx_hash
    API->>DB: INSERT event InvoicePresented (pending)
    API-->>Issuer: 200 { tx_hash }

    Note over Issuer,DB: === COLLECT (mandate mode) ===
    Issuer->>API: POST /emei/collect { invoice_id, mandate_id }
    API->>DB: INSERT into tx_queue (priority=8)
    API->>DB: INSERT event InvoicePaid (pending)
    API-->>Issuer: 200 { pending:job_42 }

    Note over Issuer,DB: === CONFIRMATION via Webhook ===
    Chain->>Alchemy: Block with InvoicePaid log
    Alchemy->>API: POST /emei/webhook (HMAC signed)
    API->>API: Verify HMAC-SHA256 signature
    API->>Redis: LPUSH emei:queue:webhook
    Redis-->>WebhookWorker: BRPOP (5s timeout)
    WebhookWorker->>WebhookWorker: Parse log topics → InvoicePaid
    WebhookWorker->>DB: upsert_confirmed_event
    WebhookWorker->>DB: insert_pending_receipt (for batching)
    WebhookWorker->>DB: enqueue_tx giveFeedback (reputation +)
```

### Collection Modes

| Mode | Trigger | Signer | Flow |
|------|---------|--------|------|
| `pay_link` | Payer calls `POST /emei/pay` | User's private key | Direct on-chain tx, user pays gas |
| `mandate` | Auto-collector detects eligible invoice | Hot wallet pool | Enqueued to tx_queue, facilitator pays gas |

---

## Transaction Queue & Wallet Pool

The tx_queue is a PostgreSQL-backed durable job queue that guarantees every enqueued transaction eventually lands on-chain (or permanently fails after max retries).

### Job State Machine

```mermaid
stateDiagram-v2
    [*] --> pending: enqueue_tx()
    pending --> assigned: claim_tx_job() [SKIP LOCKED]
    assigned --> submitted: mark_tx_submitted(tx_hash, nonce)
    submitted --> confirmed: mark_tx_confirmed(block_number)
    assigned --> pending: mark_tx_failed() [retries < max]
    submitted --> pending: tx_reaper reclaim [timeout > 120s]
    assigned --> failed: mark_tx_failed() [retries >= max]
    submitted --> failed: mark_tx_failed() [retries >= max]
    confirmed --> [*]
    failed --> [*]
```

### How the Wallet Pool Works

```mermaid
graph LR
    subgraph "tx_queue table"
        J1[Job 1 priority=10]
        J2[Job 2 priority=8]
        J3[Job 3 priority=5]
        J4[Job 4 priority=2]
    end

    subgraph "Wallet Pool (N workers)"
        W0[wallet_0<br/>0xabc...]
        W1[wallet_1<br/>0xdef...]
        W2[wallet_2<br/>0x123...]
    end

    J1 -->|"SELECT ... FOR UPDATE SKIP LOCKED"| W0
    J2 -->|claimed| W1
    J3 -->|claimed| W2
    J4 -->|waiting| J4

    W0 -->|send_transaction| RPC[Mantle RPC]
    W1 -->|send_transaction| RPC
    W2 -->|send_transaction| RPC
```

**Key design decisions:**

1. **`FOR UPDATE SKIP LOCKED`** — Multiple wallet workers poll concurrently. PostgreSQL's skip-locked ensures no two workers ever claim the same job, with zero contention.
2. **Priority ordering** — Jobs are claimed highest-priority-first (`ORDER BY priority DESC, id ASC`). Receipt batching (priority=10) > collect (priority=8) > overdue marking (priority=5) > reputation feedback (priority=3) > auto-collection (priority=2).
3. **Sequential per wallet** — Each wallet processes one job at a time. This eliminates nonce races within a single wallet.
4. **Automatic retry** — Failed jobs reset to `pending` if retries < max_retries (default 3). The tx_reaper reclaims stuck jobs every 2 minutes.

---

## Nonce Management

The system uses two nonce strategies depending on the transaction path:

### Strategy 1: Redis Atomic Nonce (Hot Wallet via `send_hot`)

Used by the legacy `ChainClient::send_hot()` path for direct hot wallet sends.

```mermaid
sequenceDiagram
    participant Service as Background Service
    participant Chain as ChainClient
    participant Redis
    participant RPC as Mantle RPC

    Service->>Chain: send_hot(to, calldata, redis)
    Chain->>RPC: get_transaction_count(hot_address)
    RPC-->>Chain: chain_nonce = 42
    Chain->>Redis: SETNX emei:nonce:0xhot (chain_nonce - 1)
    Note over Redis: Only sets if key doesn't exist
    Chain->>Redis: INCR emei:nonce:0xhot
    Redis-->>Chain: nonce = 42
    Chain->>RPC: send_transaction(nonce=42)
    
    alt Success
        RPC-->>Chain: tx_hash
        Chain-->>Service: Ok(tx_hash)
    else Nonce Too Low
        RPC-->>Chain: error: nonce too low
        Chain->>RPC: get_transaction_count(hot_address)
        RPC-->>Chain: fresh_nonce = 45
        Chain->>Redis: SET emei:nonce:0xhot 45
        Chain-->>Service: Err(nonce too low)
        Note over Service: Next cycle will use correct nonce
    else Other Failure
        RPC-->>Chain: error
        Chain->>Redis: DECR emei:nonce:0xhot
        Note over Redis: Release nonce for reuse
        Chain-->>Service: Err(...)
    end
```

### Strategy 2: Provider Auto-Fill (TX Sender Workers)

Used by `tx_sender` workers. Since each wallet processes jobs sequentially, the provider's built-in nonce management is sufficient.

```
wallet_0: Job1 (nonce auto) → wait receipt → Job2 (nonce auto) → wait receipt → ...
wallet_1: Job3 (nonce auto) → wait receipt → Job4 (nonce auto) → wait receipt → ...
```

No nonce conflicts because:
- One worker per wallet key
- Sequential processing (wait for receipt before next job)
- Provider queries chain for current nonce on each send

### Why Two Strategies?

| Path | Concurrency | Nonce Strategy | Reason |
|------|-------------|----------------|--------|
| `send_hot()` | Multiple callers, one wallet | Redis INCR | Atomic counter prevents races when multiple services call simultaneously |
| `tx_sender` workers | One worker per wallet | Provider auto-fill | Sequential processing makes atomic counters unnecessary |

---

## Webhook Pipeline

Alchemy webhooks are the primary mechanism for confirming on-chain state changes. The pipeline is designed for exactly-once processing with idempotent writes.

```mermaid
flowchart LR
    subgraph "Ingestion"
        A[Alchemy POST] -->|HMAC verify| B[/emei/webhook handler/]
        B -->|LPUSH| C[(Redis<br/>emei:queue:webhook)]
    end

    subgraph "Processing"
        C -->|BRPOP 5s| D[webhook_worker]
        D -->|parse topics| E{Event Type?}
        E -->|4 topics| F[InvoiceCreated]
        E -->|3 topics + 64B data| G[InvoicePaid]
        E -->|3 topics + 32B data| H[InvoicePresented]
        E -->|3 topics + 0B data| I[InvoiceOverdue]
    end

    subgraph "Side Effects (InvoicePaid only)"
        G -->|first confirmation| J[insert_pending_receipt]
        G -->|first confirmation| K[enqueue giveFeedback]
    end

    subgraph "Persistence"
        F --> L[(PostgreSQL<br/>upsert_confirmed_event)]
        G --> L
        H --> L
        I --> L
    end
```

### Idempotency Guarantees

1. **Webhook deduplication** — `UNIQUE(tx_hash, log_index)` constraint. Duplicate webhooks are absorbed by `ON CONFLICT DO UPDATE SET status = 'confirmed'`.
2. **Side-effect guard** — Before triggering receipt queuing or reputation feedback, the worker checks `is_event_confirmed(tx_hash, log_index)`. Side effects only fire on the first confirmation.
3. **Two payload formats** — Supports both Alchemy Address Activity and Custom Webhook (GraphQL) formats transparently.

### Webhook Signature Verification

```
HMAC-SHA256(signing_key, raw_body) == x-alchemy-signature header (hex-encoded)
```

If `ALCHEMY_WEBHOOK_SIGNING_KEY` is not set, signature verification is skipped (development mode).

---

## Background Workers

All workers are spawned as Tokio tasks with graceful shutdown via `CancellationToken`.

```mermaid
graph TB
    subgraph "Spawned on startup"
        B[receipt_batcher<br/>interval: 30s]
        C[auto_collector<br/>interval: 10s]
        S[overdue_scanner<br/>interval: 60s]
        I[event_indexer<br/>one-shot backfill]
        W[webhook_worker<br/>BRPOP loop]
        R[tx_reaper<br/>interval: 120s]
        T1[tx_sender wallet_0]
        T2[tx_sender wallet_1]
        TN[tx_sender wallet_N]
    end

    CT[CancellationToken] -.->|cancel signal| B
    CT -.->|cancel signal| C
    CT -.->|cancel signal| S
    CT -.->|cancel signal| I
    CT -.->|cancel signal| W
    CT -.->|cancel signal| R
    CT -.->|cancel signal| T1
    CT -.->|cancel signal| T2
    CT -.->|cancel signal| TN
```

| Worker | Startup Delay | Interval | Purpose |
|--------|---------------|----------|---------|
| `receipt_batcher` | 0s | 30s | Drain pending receipts → Merkle tree → post root on-chain |
| `auto_collector` | 10s | 10s | Find PRESENTED mandate-mode invoices → enqueue collect |
| `overdue_scanner` | 20s | 60s | Find overdue invoices → mark on-chain + penalize reputation |
| `event_indexer` | 10s | one-shot | Backfill DB from chain if empty, then sleep forever |
| `webhook_worker` | 0s | continuous | BRPOP from Redis, process webhook payloads |
| `tx_reaper` | 30s | 120s | Reclaim stuck jobs (assigned/submitted > 2min) |
| `tx_sender` (×N) | 0s | 2s poll | Claim jobs from tx_queue, send, confirm |

### Receipt Batcher Deep Dive

```mermaid
sequenceDiagram
    participant Batcher
    participant DB as PostgreSQL
    participant MemQ as In-Memory Queue
    participant Merkle as MerkleTree
    participant TxQueue as tx_queue

    loop Every 30 seconds
        Batcher->>DB: drain_pending_receipts(500)
        DB-->>Batcher: [hash1, hash2, ..., hashN]
        Batcher->>MemQ: drain()
        MemQ-->>Batcher: [hashA, hashB]
        
        alt Has receipts
            Batcher->>Merkle: new(all_hashes) → sort → compute root
            Merkle-->>Batcher: root (32 bytes)
            Batcher->>TxQueue: enqueue_tx(postMerkleRoot, priority=10)
            TxQueue-->>Batcher: job_id
            Note over Batcher: batch_number++
        else No receipts
            Note over Batcher: Skip cycle
        end
    end
```

---

## State Machine

### Invoice On-Chain State

```mermaid
stateDiagram-v2
    [*] --> ISSUED: createInvoice()
    ISSUED --> PRESENTED: present() [issuer only]
    PRESENTED --> PAID: pay() [payer] / collect() [hot wallet + mandate]
    PRESENTED --> OVERDUE: markOverdue() [scanner, past due]
    OVERDUE --> PAID: pay() [payer, late payment]
    PAID --> [*]
    OVERDUE --> [*]: permanent if unpaid
```

### Event Confirmation State (Database)

```mermaid
stateDiagram-v2
    [*] --> pending: API handler inserts optimistic event
    pending --> confirmed: webhook_worker upserts with ON CONFLICT
    [*] --> confirmed: webhook_worker inserts new confirmed event
    confirmed --> [*]
```

### TX Queue Job Lifecycle with Webhook Interaction

```mermaid
sequenceDiagram
    participant Service as Auto-Collector
    participant Queue as tx_queue (PostgreSQL)
    participant Sender as tx_sender wallet_0
    participant RPC as Mantle RPC
    participant Alchemy
    participant Webhook as webhook_worker
    participant DB as events table

    Service->>Queue: enqueue_tx(collectCall, priority=2)
    Service->>DB: INSERT InvoicePaid (status=pending, tx_hash=pending:job_X)
    
    Note over Sender: Polling every 2s
    Sender->>Queue: claim_tx_job("wallet_0") [SKIP LOCKED]
    Queue-->>Sender: Job { id, to, calldata }
    Sender->>RPC: send_transaction(to, calldata)
    RPC-->>Sender: tx_hash = 0xabc...
    Sender->>Queue: mark_tx_submitted(job_id, tx_hash, nonce)
    Sender->>RPC: get_transaction_receipt(tx_hash) [poll up to 60s]
    RPC-->>Sender: receipt { block_number }
    Sender->>Queue: mark_tx_confirmed(job_id, block_number)

    Note over Alchemy: Meanwhile, Alchemy detects the log
    Alchemy->>Webhook: POST webhook payload
    Webhook->>DB: upsert_confirmed_event (tx_hash=0xabc, status=confirmed)
    Note over DB: Pending event now confirmed with real tx_hash
```

---

## Scaling: 1 Agent vs 1000 Agents

### Single Agent Scenario

```
Agent A creates invoice → presents → auto-collector finds mandate → collects

Timeline:
  t=0s   POST /emei/invoice (user signs, direct to chain)
  t=3s   POST /emei/present (user signs, direct to chain)
  t=10s  auto_collector detects PRESENTED + mandate → enqueue_tx
  t=12s  tx_sender claims job → sends → waits receipt
  t=18s  tx confirmed on-chain
  t=20s  Alchemy webhook → confirmed in DB → receipt queued
  t=30s  receipt_batcher posts Merkle root

Total: ~30s from present to settlement proof anchored
```

With 1 wallet, 1 agent: the system processes jobs sequentially. No contention, no nonce issues. The tx_sender polls every 2s, so worst-case latency from enqueue to send is 2s.

### 1000 Agents Scenario

```
1000 agents each create + present invoices simultaneously
→ 1000 collect jobs enqueued to tx_queue within seconds
→ Wallet pool processes them in parallel
```

```mermaid
graph TB
    subgraph "1000 Agents"
        A1[Agent 1]
        A2[Agent 2]
        AN[Agent 1000]
    end

    subgraph "API (Axum, multi-threaded)"
        H1[Handler]
        H2[Handler]
        HN[Handler]
    end

    subgraph "tx_queue (PostgreSQL)"
        Q[1000 pending jobs<br/>sorted by priority DESC, id ASC]
    end

    subgraph "Wallet Pool (5 wallets)"
        W0[wallet_0: Job 1 → Job 6 → Job 11...]
        W1[wallet_1: Job 2 → Job 7 → Job 12...]
        W2[wallet_2: Job 3 → Job 8 → Job 13...]
        W3[wallet_3: Job 4 → Job 9 → Job 14...]
        W4[wallet_4: Job 5 → Job 10 → Job 15...]
    end

    A1 --> H1
    A2 --> H2
    AN --> HN
    H1 --> Q
    H2 --> Q
    HN --> Q
    Q --> W0
    Q --> W1
    Q --> W2
    Q --> W3
    Q --> W4
```

**Throughput calculation (5 wallets):**

| Metric | Value |
|--------|-------|
| Avg block time (Mantle) | ~2s |
| Tx confirmation time | ~6s (send + 2 confirmations) |
| Jobs per wallet per minute | ~10 |
| Total throughput (5 wallets) | ~50 tx/min |
| Time to drain 1000 jobs | ~20 minutes |
| Time to drain 1000 jobs (10 wallets) | ~10 minutes |

**Why this doesn't collapse under load:**

1. **No lock contention** — `SKIP LOCKED` means wallet workers never block each other. If wallet_0 is processing job 1, wallet_1 instantly claims job 2.
2. **No nonce races** — Each wallet is sequential. Wallet_0 always waits for its current tx receipt before claiming the next job.
3. **Backpressure is natural** — If the queue grows faster than wallets can drain it, jobs simply wait. No memory pressure (it's all in PostgreSQL).
4. **Priority ensures fairness** — High-priority jobs (receipt batching, explicit collects) always go first, even under load.
5. **Webhook processing is independent** — The webhook_worker runs on its own Redis queue. 1000 webhook payloads are processed sequentially but quickly (no chain calls, just DB writes).

### Scaling Levers

| Lever | How | Impact |
|-------|-----|--------|
| Add wallet keys | `EMEI_HOT_WALLET_KEYS=key1,key2,...` | Linear throughput increase |
| Increase PostgreSQL connections | `max_connections` in pool | More concurrent claims |
| Redis connection pooling | Already uses ConnectionManager | Handles webhook burst |
| Horizontal scaling | Multiple facilitator instances | All share same tx_queue via SKIP LOCKED |

---

## Database Schema

```mermaid
erDiagram
    events {
        bigserial id PK
        text event_type
        bigint block_number
        text tx_hash
        integer log_index
        bigint timestamp
        bigint invoice_id
        text payer
        text issuer
        text amount
        text params
        text status "pending | confirmed"
    }

    tx_queue {
        bigserial id PK
        text to_address
        bytea calldata
        smallint priority
        text status "pending | assigned | submitted | confirmed | failed"
        text wallet_id
        bigint nonce
        text tx_hash
        bigint submitted_at
        bigint confirmed_at
        bigint block_number
        text error
        smallint retries
        smallint max_retries "default 3"
        bigint created_at
        bigint assigned_at
        text source
    }

    pending_receipts {
        bigserial id PK
        bytea receipt_hash "32 bytes"
        bigint invoice_id
        bigint created_at
    }

    pending_txs {
        bigserial id PK
        text tx_hash UK
        text sender
        bigint nonce
        bigint submitted_at
        bigint confirmed_at
        text status "pending | confirmed"
    }

    indexer_state {
        text key PK
        text value
    }

    events ||--o{ pending_receipts : "InvoicePaid triggers"
    tx_queue ||--o{ events : "confirmed tx updates"
```

### Key Indexes

```sql
-- Fast job claiming (pending jobs by priority)
CREATE INDEX idx_tx_queue_pending ON tx_queue(priority DESC, id ASC) WHERE status = 'pending';

-- Wallet assignment lookup
CREATE INDEX idx_tx_queue_assigned ON tx_queue(wallet_id, status) WHERE status = 'assigned';

-- Event lookups
CREATE INDEX idx_events_payer ON events(payer);
CREATE INDEX idx_events_issuer ON events(issuer);
CREATE INDEX idx_events_invoice_id ON events(invoice_id);
CREATE INDEX idx_events_block_number ON events(block_number DESC);
```

---

## API Reference

### Invoice Lifecycle

| Endpoint | Method | Auth | Description |
|----------|--------|------|-------------|
| `/emei/invoice` | POST | `X-Private-Key` | Create a new invoice on-chain |
| `/emei/present` | POST | `X-Private-Key` | Present invoice to payer |
| `/emei/pay` | POST | `X-Private-Key` | Pay invoice directly (payer-initiated) |
| `/emei/collect` | POST | None | Collect via mandate (hot wallet, queued) |

### Mandate Management

| Endpoint | Method | Auth | Description |
|----------|--------|------|-------------|
| `/emei/mandate` | POST | `X-Private-Key` | Create spending mandate |
| `/emei/mandate/{id}` | DELETE | `X-Private-Key` | Revoke mandate |

### Query & Verification

| Endpoint | Method | Auth | Description |
|----------|--------|------|-------------|
| `/emei/invoice/{id}` | GET | None | Get invoice details from chain |
| `/emei/statement` | GET | None | Query events by payer (paginated) |
| `/emei/reputation/{addr}` | GET | None | Get on-chain reputation score |
| `/emei/balance/{addr}` | GET | None | Get vault balance + accrued yield |
| `/emei/verify/{id}` | GET | None | Verify receipt Merkle inclusion |
| `/emei/paylink/{id}` | GET | None | Get pre-encoded pay-link calldata |

### Identity & Withdrawal

| Endpoint | Method | Auth | Description |
|----------|--------|------|-------------|
| `/emei/register` | POST | `X-Private-Key` | Register identity (ERC-8004) |
| `/emei/withdraw` | POST | `X-Private-Key` | Withdraw from settlement vault |

### Operations & Monitoring

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check (RPC + DB status) |
| `/emei/webhook` | POST | Alchemy webhook receiver (HMAC) |
| `/emei/ops` | GET | HTML ops dashboard (auto-refresh) |
| `/emei/ops/status` | GET | JSON system internals |
| `/emei/ops/reset` | POST | Truncate all tables (danger) |
| `/emei/public/stats` | GET | Aggregated protocol stats |
| `/emei/public/events` | GET | Recent events (paginated) |
| `/emei/public/agents` | GET | Known agents with enriched data |
| `/emei/public/mandates` | GET | Active mandates across agents |

---

## Configuration

All configuration is via environment variables (loaded from `.env` via dotenvy).

### Required

| Variable | Description |
|----------|-------------|
| `EMEI_RPC_URL` | Mantle Sepolia RPC endpoint |
| `EMEI_HOT_WALLET_KEY` | Primary hot wallet private key (hex, 32 bytes) |
| `EMEI_INVOICE_ADDRESS` | EMEIInvoice contract address |
| `EMEI_MANDATE_ADDRESS` | EMEIMandate contract address |
| `EMEI_SETTLEMENT_ADDRESS` | EMEISettlement contract address |
| `EMEI_RECEIPT_ADDRESS` | EMEIReceipt contract address |
| `EMEI_BAY8004_ADDRESS` | Bay8004 reputation contract address |
| `EMEI_ERC8004_ADDRESS` | MockERC8004 identity registry address |
| `DATABASE_URL` | PostgreSQL connection string |
| `REDIS_URL` | Redis connection string |

### Optional

| Variable | Default | Description |
|----------|---------|-------------|
| `EMEI_HOT_WALLET_KEYS` | — | Additional wallet keys (comma-separated) for pool |
| `ALCHEMY_WEBHOOK_SIGNING_KEY` | — | HMAC key for webhook verification |
| `EMEI_BATCH_INTERVAL` | `30` | Seconds between receipt batching cycles |
| `EMEI_COLLECT_INTERVAL` | `10` | Seconds between auto-collection scans |
| `EMEI_OVERDUE_INTERVAL` | `60` | Seconds between overdue scans |
| `DEMO_AGENTS` | — | Comma-separated `label:address` pairs for dashboard |

---

## Running

```bash
# Install dependencies
cargo build --release

# Set up environment
cp .env.example .env
# Edit .env with your values

# Run migrations (automatic on startup)
# PostgreSQL and Redis must be running

# Start the server
cargo run --release --bin emei-server
```

The server binds to `0.0.0.0:8080` and spawns all background workers automatically.

### Health Check

```bash
curl http://localhost:8080/health
```

```json
{
  "status": "healthy",
  "rpc_reachable": true,
  "db_writable": true,
  "last_indexed_id": 0,
  "pending_receipts": 0,
  "version": "0.1.0",
  "chain_id": 5003
}
```

---

## Tech Stack

| Component | Technology |
|-----------|-----------|
| Language | Rust (2021 edition) |
| HTTP Framework | Axum |
| Async Runtime | Tokio (multi-threaded) |
| Blockchain | Alloy-rs (provider, signer, sol-types) |
| Database | PostgreSQL (sqlx, async) |
| Queue/Cache | Redis (connection-manager, async) |
| Webhook Auth | HMAC-SHA256 (hmac + sha2 crates) |
| Cryptography | Keccak256 (alloy-primitives) |
| Testing | proptest (property-based), tokio test-util |

---

## Contract Interactions

| Contract | Key Functions | Used By |
|----------|--------------|---------|
| EMEIInvoice | createInvoice, present, pay, collect, markOverdue, getInvoice | API routes, collector, scanner |
| EMEIMandate | createMandate, revokeMandate, getMandatesByPayer | API routes, collector |
| EMEISettlement | withdraw, getVaultBalance, getAccruedYield | API routes, dashboard |
| EMEIReceipt | postMerkleRoot, getLatestBatch, getMerkleRoot, verifyInclusion | Batcher, receipt verification |
| Bay8004 | scoreOf, giveFeedback | Webhook worker, scanner, dashboard |
| MockERC8004 | register | Identity registration |
