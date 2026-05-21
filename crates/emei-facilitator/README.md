# EMEI Facilitator — REST API

> Backend service for the EMEI programmable invoicing protocol. Connects agents and humans to on-chain invoice contracts on Mantle Sepolia via a simple JSON API.

## Quick Start

```bash
# Set required env vars
export EMEI_RPC_URL=https://rpc.sepolia.mantle.xyz
export EMEI_HOT_WALLET_KEY=0xYOUR_PRIVATE_KEY
export EMEI_INVOICE_ADDRESS=0xC35f709255D7199394655F16008e8d1A3AD80005
export EMEI_MANDATE_ADDRESS=0xF48C3bd4FE046629A9c12A39693f39c297893bD8
export EMEI_SETTLEMENT_ADDRESS=0xfdCb7bA077069A7Da44711Ee6bdB49174AFA4dD0
export EMEI_RECEIPT_ADDRESS=0x558a20766d5998765B056597b8b78fe1914f3969
export EMEI_BAY8004_ADDRESS=0xE61B57D84fb55E2601ab47B83c367612E348d409
export EMEI_ERC8004_ADDRESS=0x4B560970423B08632bC2Aa31D0a70e29e66Fca37

# Build
cargo build -p emei-facilitator

# Test
cargo test -p emei-facilitator
```

## Authentication

Write endpoints require the caller's private key in the `X-Private-Key` header:

```
X-Private-Key: 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
```

The facilitator signs and submits the transaction on behalf of the caller. Read endpoints (GET) require no authentication.

---

## API Reference

### Identity

#### Register Identity

```
POST /emei/register
```

Register an ERC-8004 identity in the reputation registry.

**Request:**
```json
{
  "initial_score": 500
}
```

`initial_score` is optional (defaults to 100).

**Response (201):**
```json
{
  "tx_hash": "0xabc123..."
}
```

---

### Invoices

#### Create Invoice

```
POST /emei/invoice
```

**Request:**
```json
{
  "payer": "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb0",
  "amount": "1000000000000000000000",
  "asset": "0xb4C74657Ef45AA95E91BBac1db7f9C964D1cAeAD",
  "line_items": [
    {
      "description": "Data processing - May 2026",
      "amount": "1000000000000000000000",
      "category": "data-services"
    }
  ],
  "terms": {
    "term_type": "net_n_days",
    "net_days": 7
  },
  "collection_mode": "mandate"
}
```

**Terms options:**
- `"term_type": "due_on_receipt"` — due immediately
- `"term_type": "net_n_days", "net_days": 7` — due in N days (1-365)
- `"term_type": "milestones", "milestones": [{"amount": "500...", "due_date": 1716000000, "description": "Phase 1"}]`

**Collection mode:** `"mandate"` or `"pay_link"`

**Response (201):**
```json
{
  "tx_hash": "0xdef456..."
}
```

#### Present Invoice

```
POST /emei/present
```

**Request:**
```json
{
  "invoice_id": 1
}
```

**Response (200):**
```json
{
  "tx_hash": "0x789abc..."
}
```

#### Pay Invoice

```
POST /emei/pay
```

Payer calls this to settle an invoice directly.

**Request:**
```json
{
  "invoice_id": 1
}
```

**Response (200):**
```json
{
  "tx_hash": "0xfed321..."
}
```

#### Collect via Mandate

```
POST /emei/collect
```

Triggers auto-collection using the facilitator's hot wallet. No `X-Private-Key` needed.

**Request:**
```json
{
  "invoice_id": 1,
  "mandate_id": 1
}
```

**Response (200):**
```json
{
  "tx_hash": "0x111222..."
}
```

#### Get Invoice

```
GET /emei/invoice/{id}
```

**Response (200):**
```json
{
  "invoice_id": "1",
  "issuer": "0x1234...",
  "payer": "0xabcd...",
  "amount": "1000000000000000000000",
  "asset": "0xb4C7...",
  "status": "PRESENTED",
  "collection_mode": "mandate",
  "settlement_proof": "0x0000...0000",
  "presented_at": "1716000000",
  "created_at": "1715999000"
}
```

---

### Mandates

#### Create Mandate

```
POST /emei/mandate
```

**Request:**
```json
{
  "spend_cap": "5000000000000000000000",
  "approved_counterparties": ["0x1234..."],
  "approved_categories": ["data-services", "compute"],
  "valid_from": 1716000000,
  "valid_until": 1718592000
}
```

**Response (201):**
```json
{
  "tx_hash": "0xaaa..."
}
```

#### Revoke Mandate

```
DELETE /emei/mandate/{id}
```

**Response (200):**
```json
{
  "tx_hash": "0xbbb..."
}
```

---

### Pay-Link (x402 Fallback)

#### Get Pay-Link Info

```
GET /emei/paylink/{id}
```

Returns everything a frontend needs to render the pay page and prompt wallet signatures.

**Response (200):**
```json
{
  "invoice_id": 1,
  "issuer": "0x1234...",
  "payer": "0xabcd...",
  "amount": "1000000000000000000000",
  "asset": "0xb4C7...",
  "status": "PRESENTED",
  "settlement_contract": "0xfdCb...",
  "invoice_contract": "0xC35f...",
  "approve_calldata": "0x095ea7b3...",
  "pay_calldata": "0x...",
  "approve_to": "0xb4C7...",
  "pay_to": "0xC35f..."
}
```

**Frontend usage:**
1. Fetch this endpoint
2. Display invoice details to payer
3. Prompt wallet to sign `approve_calldata` → send to `approve_to`
4. Prompt wallet to sign `pay_calldata` → send to `pay_to`
5. Invoice is paid

---

### Queries

#### Reputation Score

```
GET /emei/reputation/{address}
```

**Response (200):**
```json
{
  "address": "0x1234...",
  "score": 500
}
```

#### Vault Balance

```
GET /emei/balance/{address}
```

**Response (200):**
```json
{
  "balance": "1050000000000000000000",
  "accrued_yield": "50000000000000000000"
}
```

#### Statement (Reconciliation)

```
GET /emei/statement?payer=0xabcd...&status=InvoicePaid&from=1716000000&to=1718000000&limit=50&offset=0
```

All query params except `payer` are optional.

**Response (200):**
```json
[
  {
    "event_type": "InvoicePaid",
    "block_number": 38900000,
    "tx_hash": "0x...",
    "log_index": 0,
    "timestamp": 1716500000,
    "invoice_id": 1,
    "payer": "0xabcd...",
    "issuer": "0x1234...",
    "amount": "1000000000000000000000",
    "params": "{...}"
  }
]
```

#### Verify Receipt

```
GET /emei/verify/{invoice_id}
```

**Response (200):**
```json
{
  "verified": true,
  "batch_number": 5
}
```

---

### Withdrawal

#### Withdraw from Vault

```
POST /emei/withdraw
```

**Request:**
```json
{
  "amount": "500000000000000000000"
}
```

**Response (200):**
```json
{
  "tx_hash": "0xccc..."
}
```

---

## Error Responses

All errors return structured JSON:

```json
{
  "error_code": "VALIDATION_ERROR",
  "message": "validation error: amount: must be a non-zero positive integer",
  "resource": "amount"
}
```

**Status codes:**

| Code | Meaning |
|------|---------|
| 400 | Validation error (bad input) |
| 401 | Missing or invalid `X-Private-Key` |
| 402 | Insufficient funds / allowance |
| 403 | Unauthorized (wrong caller or low reputation) |
| 404 | Resource not found |
| 409 | State conflict (invalid status transition) |
| 422 | Business logic error (mandate expired, cap exceeded) |
| 500 | Internal server error |
| 502 | RPC error (chain unreachable) |
| 504 | RPC timeout (>5s) |

**Contract reverts** are decoded into human-readable messages:
```json
{
  "error_code": "CONTRACT_REVERT",
  "message": "contract revert: ReputationTooLow(account=0x..., score=30, threshold=50)"
}
```

---

## Background Services

These run automatically when the server starts:

| Service | Interval | What it does |
|---------|----------|--------------|
| Receipt Batcher | 30s | Builds Merkle tree from pending receipts, posts root on-chain |
| Auto-Collector | 10s | Matches due invoices against mandates, triggers collection |
| Overdue Scanner | 60s | Marks past-due invoices as OVERDUE |
| Event Indexer | Continuous | Indexes contract events into SQLite for /statement queries |

---

## Configuration

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `EMEI_RPC_URL` | ✅ | — | Mantle Sepolia RPC endpoint |
| `EMEI_HOT_WALLET_KEY` | ✅ | — | Private key for facilitator-signed txs |
| `EMEI_INVOICE_ADDRESS` | ✅ | — | EMEIInvoice contract address |
| `EMEI_MANDATE_ADDRESS` | ✅ | — | EMEIMandate contract address |
| `EMEI_SETTLEMENT_ADDRESS` | ✅ | — | EMEISettlement contract address |
| `EMEI_RECEIPT_ADDRESS` | ✅ | — | EMEIReceipt contract address |
| `EMEI_BAY8004_ADDRESS` | ✅ | — | Bay8004 contract address |
| `EMEI_ERC8004_ADDRESS` | ✅ | — | MockERC8004 contract address |
| `EMEI_SQLITE_PATH` | ❌ | `./emei.db` | SQLite database path |
| `EMEI_BATCH_INTERVAL` | ❌ | `30` | Receipt batch interval (seconds) |
| `EMEI_COLLECT_INTERVAL` | ❌ | `10` | Auto-collect interval (seconds) |
| `EMEI_OVERDUE_INTERVAL` | ❌ | `60` | Overdue scan interval (seconds) |

---

## Example: Full Invoice Lifecycle

```bash
# 1. Register both parties
curl -X POST http://localhost:8080/emei/register \
  -H "Content-Type: application/json" \
  -H "X-Private-Key: 0xISSUER_KEY" \
  -d '{"initial_score": 500}'

curl -X POST http://localhost:8080/emei/register \
  -H "Content-Type: application/json" \
  -H "X-Private-Key: 0xPAYER_KEY" \
  -d '{"initial_score": 500}'

# 2. Create invoice
curl -X POST http://localhost:8080/emei/invoice \
  -H "Content-Type: application/json" \
  -H "X-Private-Key: 0xISSUER_KEY" \
  -d '{
    "payer": "0xPAYER_ADDRESS",
    "amount": "1000000000000000000000",
    "asset": "0xb4C74657Ef45AA95E91BBac1db7f9C964D1cAeAD",
    "line_items": [{"description": "Service", "amount": "1000000000000000000000", "category": "services"}],
    "terms": {"term_type": "net_n_days", "net_days": 7},
    "collection_mode": "pay_link"
  }'

# 3. Present invoice
curl -X POST http://localhost:8080/emei/present \
  -H "Content-Type: application/json" \
  -H "X-Private-Key: 0xISSUER_KEY" \
  -d '{"invoice_id": 1}'

# 4. Pay invoice
curl -X POST http://localhost:8080/emei/pay \
  -H "Content-Type: application/json" \
  -H "X-Private-Key: 0xPAYER_KEY" \
  -d '{"invoice_id": 1}'

# 5. Check result
curl http://localhost:8080/emei/invoice/1
curl http://localhost:8080/emei/balance/0xISSUER_ADDRESS
```

---

## Architecture

```
crates/emei-facilitator/
├── src/
│   ├── lib.rs              # Public API: emei_router() + start_services()
│   ├── config.rs           # EmeiConfig from env vars
│   ├── state.rs            # AppState + ReceiptQueue
│   ├── error.rs            # EmeiError + contract revert decoding
│   ├── chain.rs            # ChainClient trait + AlloyChainClient
│   ├── signing.rs          # UserSigner extractor (X-Private-Key header)
│   ├── types.rs            # Request/response structs + validation
│   ├── merkle.rs           # Keccak256 Merkle tree (OZ-compatible)
│   ├── contracts/          # alloy sol! bindings for all 6 contracts
│   ├── routes/             # 14 Axum route handlers
│   ├── services/           # 4 background services (tokio tasks)
│   └── db/                 # SQLite schema + queries (WAL mode)
└── abi/                    # Contract ABI JSON files
```
