# EMEI Demo Agents

> Two TypeScript bots that bill each other on-chain through the EMEI Facilitator API, creating a live demo of the protocol.

## How It Works

```
signal-bot (issuer)          Facilitator           trader-bot (payer)
     │                           │                       │
     │  Every 5 min:             │                       │
     │  POST /emei/invoice ─────►│──► on-chain tx        │
     │  POST /emei/present ─────►│──► on-chain tx        │
     │                           │                       │
     │                    Auto-Collector (10s):           │
     │                    matches invoice + mandate       │
     │                    calls collect() ──────────────►│ (mUSD debited)
     │◄──────────────── vault balance increases          │
     │                           │                       │
```

## Setup

```bash
# 1. Copy env file and fill in your keys
cp .env.example .env

# 2. Install dependencies
npm install

# 3. Build TypeScript
npm run build

# 4. Fund wallets and register identities (run once)
npm run seed

# 5. Create the mandate (run once)
npm run trader

# 6. Start the invoice loop (runs forever)
npm run signal
```

## Docker (Full Stack)

```bash
# Brings up: Facilitator → seed → trader-bot → signal-bot
docker compose -f docker-compose.demo.yml up --build
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `FACILITATOR_URL` | Facilitator API URL (default: `http://localhost:8080`) |
| `RPC_URL` | Mantle Sepolia RPC endpoint |
| `CHAIN_ID` | Chain ID (5003) |
| `SIGNAL_BOT_PK` | Signal bot private key (issuer) |
| `TRADER_BOT_PK` | Trader bot private key (payer) |
| `MOCK_MUSD_ADDR` | MockmUSD token contract address |
| `EMEI_SETTLEMENT_ADDRESS` | Settlement contract (for approve) |
| `INTERVAL_SECONDS` | Seconds between invoices (default: 300) |

## What Each Script Does

- **seed.ts** — Mints mUSD, approves Settlement, registers both identities
- **trader-bot.ts** — Creates a 30-day mandate authorizing signal-bot
- **signal-bot.ts** — Loops forever issuing + presenting 1 mUSD invoices
