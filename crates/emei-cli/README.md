# EMEI CLI — Agent-Facing Invoicing Client

> Thin HTTP client for the EMEI Facilitator API. All output is structured JSON for machine parsing by agent runtimes.

## Installation

```bash
cargo build -p emei-cli --release
# Binary: target/release/emei
```

Or run directly:

```bash
cargo run -p emei-cli -- <command>
```

## Configuration

Two environment variables control the CLI:

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `EMEI_API_URL` | No | `http://localhost:8080` | Facilitator server URL |
| `EMEI_PRIVATE_KEY` | Yes* | — | Hex-encoded private key (with or without `0x` prefix) |

\* Required for all write operations (register, create, pay, etc.)

```bash
export EMEI_API_URL=http://localhost:8080
export EMEI_PRIVATE_KEY=0xYOUR_PRIVATE_KEY
```

Or source the provided env file:

```bash
source .env.cli
```

---

## Output Format

Every command outputs a single JSON line. Exit code `0` on success, non-zero on failure.

**Success:**
```json
{"success": true, "data": { ... }}
```

**Failure:**
```json
{"success": false, "error": {"code": "API_ERROR", "message": "VALIDATION_ERROR: validation error: amount: must be a non-zero positive integer"}}
```

---

## Amount Handling

Amounts are specified in human-readable token units and automatically converted to wei (18 decimals):

| Input | Interpreted As |
|-------|---------------|
| `"100"` | 100 tokens → `100000000000000000000` wei |
| `"0.1"` | 0.1 tokens → `100000000000000000` wei |
| `"1.5"` | 1.5 tokens → `1500000000000000000` wei |
| `"1000000000000000000000"` | Already in wei (>1e15), passed through |

---

## Commands

### Wallet

#### Register Identity

```bash
emei wallet create [--score <INITIAL_SCORE>]
```

Registers an ERC-8004 identity in the reputation registry.

| Flag | Default | Description |
|------|---------|-------------|
| `--score` | `100` | Initial reputation score |

**Example:**
```bash
emei wallet create --score 500
```

**Response:**
```json
{"success": true, "data": {"tx_hash": "0xabc123..."}}
```

---

### Invoice

#### Create Invoice

```bash
emei invoice create \
  --payer <ADDRESS> \
  --amount <AMOUNT> \
  --asset <TOKEN_ADDRESS> \
  [--category <CATEGORY>] \
  [--description <DESCRIPTION>] \
  [--terms <TERMS_TYPE>] \
  [--net-days <DAYS>] \
  [--mode <COLLECTION_MODE>]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--payer` | — | Payer's Ethereum address |
| `--amount` | — | Amount in tokens (auto-converted to wei) |
| `--asset` | — | ERC-20 token contract address |
| `--category` | `services` | Line item category |
| `--description` | `Service` | Line item description |
| `--terms` | `due_on_receipt` | `due_on_receipt`, `net_n_days`, or `milestones` |
| `--net-days` | `7` | Days until due (for `net_n_days`) |
| `--mode` | `pay_link` | `mandate` or `pay_link` |

**Example:**
```bash
emei invoice create \
  --payer 0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb0 \
  --amount 100 \
  --asset 0xb4C74657Ef45AA95E91BBac1db7f9C964D1cAeAD \
  --category "data-services" \
  --description "Data processing - May 2026" \
  --terms net_n_days \
  --net-days 7 \
  --mode mandate
```

**Response:**
```json
{"success": true, "data": {"tx_hash": "0xdef456..."}}
```

#### Present Invoice

```bash
emei invoice present <ID>
```

Transitions an invoice from ISSUED → PRESENTED.

**Example:**
```bash
emei invoice present 1
```

**Response:**
```json
{"success": true, "data": {"tx_hash": "0x789abc..."}}
```

#### Pay Invoice

```bash
emei invoice pay <ID>
```

Payer settles an invoice directly.

**Example:**
```bash
emei invoice pay 1
```

**Response:**
```json
{"success": true, "data": {"tx_hash": "0xfed321..."}}
```

#### Get Invoice

```bash
emei invoice get <ID>
```

Fetch invoice details from the chain.

**Example:**
```bash
emei invoice get 1
```

**Response:**
```json
{"success": true, "data": {"invoice_id": "1", "issuer": "0x1234...", "payer": "0xabcd...", "amount": "1000000000000000000000", "asset": "0xb4C7...", "status": "PRESENTED", "collection_mode": "mandate", "settlement_proof": "0x0000...0000", "presented_at": "1716000000", "created_at": "1715999000"}}
```

#### List Invoices

```bash
emei invoice list [--from <START_ID>] [--to <END_ID>]
```

Fetches invoices by ID range. Skips IDs that don't exist.

| Flag | Default | Description |
|------|---------|-------------|
| `--from` | `1` | Start invoice ID |
| `--to` | `10` | End invoice ID (inclusive) |

**Example:**
```bash
emei invoice list --from 1 --to 5
```

**Response:**
```json
{"success": true, "data": [{"invoice_id": "1", "issuer": "0x...", ...}, {"invoice_id": "2", ...}]}
```

---

### Mandate

#### Create Mandate

```bash
emei mandate create \
  --spend-cap <AMOUNT> \
  --counterparties <ADDR1,ADDR2,...> \
  [--categories <CAT1,CAT2,...>] \
  --valid-from <UNIX_TIMESTAMP> \
  --valid-until <UNIX_TIMESTAMP>
```

| Flag | Default | Description |
|------|---------|-------------|
| `--spend-cap` | — | Maximum spend in tokens (auto-converted to wei) |
| `--counterparties` | — | Comma-separated approved payee addresses |
| `--categories` | `services` | Comma-separated approved invoice categories |
| `--valid-from` | — | Unix timestamp for mandate start |
| `--valid-until` | — | Unix timestamp for mandate expiry |

**Example:**
```bash
emei mandate create \
  --spend-cap 5000 \
  --counterparties 0x1234567890abcdef1234567890abcdef12345678 \
  --categories "data-services,compute" \
  --valid-from 1716000000 \
  --valid-until 1718592000
```

**Response:**
```json
{"success": true, "data": {"tx_hash": "0xaaa..."}}
```

#### Revoke Mandate

```bash
emei mandate revoke <ID>
```

**Example:**
```bash
emei mandate revoke 1
```

**Response:**
```json
{"success": true, "data": {"tx_hash": "0xbbb..."}}
```

---

### Collect

```bash
emei collect <INVOICE_ID> <MANDATE_ID>
```

Triggers collection of an invoice via a mandate. Uses the facilitator's hot wallet (no private key needed from the caller for the on-chain tx, but the header is still sent).

**Example:**
```bash
emei collect 1 1
```

**Response:**
```json
{"success": true, "data": {"tx_hash": "0x111222..."}}
```

---

### Balance

```bash
emei balance [ADDRESS]
```

Check vault balance and accrued yield. If no address is provided, defaults to `0x0`.

**Example:**
```bash
emei balance 0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb0
```

**Response:**
```json
{"success": true, "data": {"balance": "1050000000000000000000", "accrued_yield": "50000000000000000000"}}
```

---

### Reputation

```bash
emei reputation <ADDRESS>
```

Query the Bay8004 reputation score for an address.

**Example:**
```bash
emei reputation 0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb0
```

**Response:**
```json
{"success": true, "data": {"address": "0x742d35cc6634c0532925a3b844bc9e7595f0beb0", "score": 500}}
```

---

### Withdraw

```bash
emei withdraw <AMOUNT>
```

Withdraw funds from the settlement vault.

**Example:**
```bash
emei withdraw 50
```

**Response:**
```json
{"success": true, "data": {"tx_hash": "0xccc..."}}
```

---

### Pay (Shortcut)

```bash
emei pay <INVOICE_ID>
```

Shortcut for `emei invoice pay`. Identical behavior.

**Example:**
```bash
emei pay 1
```

**Response:**
```json
{"success": true, "data": {"tx_hash": "0xfed321..."}}
```

---

## Full Lifecycle Example

```bash
# Source environment
source .env.cli

# 1. Register identity
emei wallet create --score 500

# 2. Create an invoice (issuer → payer)
emei invoice create \
  --payer 0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb0 \
  --amount 100 \
  --asset 0xb4C74657Ef45AA95E91BBac1db7f9C964D1cAeAD \
  --terms net_n_days --net-days 7 \
  --mode mandate

# 3. Present the invoice
emei invoice present 1

# 4. Create a mandate (payer side — switch EMEI_PRIVATE_KEY)
export EMEI_PRIVATE_KEY=0xPAYER_KEY
emei mandate create \
  --spend-cap 1000 \
  --counterparties 0xISSUER_ADDRESS \
  --categories services \
  --valid-from 1716000000 \
  --valid-until 1718592000

# 5. Collect via mandate
emei collect 1 1

# 6. Check balance
emei balance 0xISSUER_ADDRESS

# 7. Withdraw
export EMEI_PRIVATE_KEY=0xISSUER_KEY
emei withdraw 50
```

---

## Error Codes

| Code | Meaning |
|------|---------|
| `API_ERROR` | Generic wrapper — check the message for details |
| `VALIDATION_ERROR` | Bad input (invalid address, zero amount, etc.) |
| `MISSING_AUTH` | `EMEI_PRIVATE_KEY` not set or `X-Private-Key` header missing |
| `INVALID_AUTH` | Key is malformed or not 32 bytes |
| `INSUFFICIENT_FUNDS` | Not enough tokens or gas |
| `UNAUTHORIZED` | Wrong caller or reputation too low |
| `NOT_FOUND` | Invoice or mandate doesn't exist |
| `STATE_CONFLICT` | Invalid status transition (e.g., paying an already-paid invoice) |
| `BUSINESS_LOGIC_ERROR` | Mandate expired, cap exceeded, etc. |
| `RPC_ERROR` | Chain node unreachable |
| `RPC_TIMEOUT` | Chain call exceeded timeout |

---

## Architecture

```
crates/emei-cli/
├── Cargo.toml
├── README.md
└── src/
    └── main.rs    # Single-file CLI: clap parsing, HTTP client, JSON output
```

The CLI is intentionally minimal — a thin HTTP wrapper over the Facilitator API. All business logic, validation, and chain interaction lives in the facilitator backend.
