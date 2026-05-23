/**
 * Shared utilities for EMEI demo agents.
 * Env loading, fetch wrapper, viem client setup.
 */

import "dotenv/config";

import {
  createPublicClient,
  createWalletClient,
  http,
  type Address,
  type Hex,
  type Chain,
} from "viem";
import { privateKeyToAccount } from "viem/accounts";

// ─── Environment ─────────────────────────────────────────────────────────────

export function env(key: string, fallback?: string): string {
  const val = process.env[key] ?? fallback;
  if (!val) {
    console.error(`[env] Missing required variable: ${key}`);
    process.exit(1);
  }
  return val;
}

export const FACILITATOR_URL = env("FACILITATOR_URL", "http://localhost:8080");
export const RPC_URL = env("RPC_URL", "https://rpc.sepolia.mantle.xyz");
export const CHAIN_ID = parseInt(env("CHAIN_ID", "5003"));
export const MOCK_MUSD_ADDR = env("MOCK_MUSD_ADDR") as Address;
export const INTERVAL_SECONDS = parseInt(env("INTERVAL_SECONDS", "300"));

export const SIGNAL_BOT_PK = env("SIGNAL_BOT_PK") as Hex;
export const TRADER_BOT_PK = env("TRADER_BOT_PK") as Hex;

export const signalBotAccount = privateKeyToAccount(SIGNAL_BOT_PK);
export const traderBotAccount = privateKeyToAccount(TRADER_BOT_PK);

// ─── Chain definition ────────────────────────────────────────────────────────

export const mantleSepolia: Chain = {
  id: CHAIN_ID,
  name: "Mantle Sepolia",
  nativeCurrency: { name: "MNT", symbol: "MNT", decimals: 18 },
  rpcUrls: {
    default: { http: [RPC_URL] },
  },
  blockExplorers: {
    default: { name: "Mantlescan", url: "https://explorer.sepolia.mantle.xyz" },
  },
};

// ─── Viem clients ────────────────────────────────────────────────────────────

export const publicClient = createPublicClient({
  chain: mantleSepolia,
  transport: http(RPC_URL),
});

export function walletClient(pk: Hex) {
  const account = privateKeyToAccount(pk);
  return createWalletClient({
    account,
    chain: mantleSepolia,
    transport: http(RPC_URL),
  });
}

// ─── Facilitator API wrapper ─────────────────────────────────────────────────

export async function facilitatorPost(
  path: string,
  body: Record<string, unknown>,
  privateKey: Hex
): Promise<Record<string, unknown>> {
  const url = `${FACILITATOR_URL}${path}`;
  const res = await fetch(url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-Private-Key": privateKey,
    },
    body: JSON.stringify(body),
  });

  const data = await res.json();

  if (!res.ok) {
    const msg = (data as any).message ?? JSON.stringify(data);
    throw new Error(`[${res.status}] ${path}: ${msg}`);
  }

  return data as Record<string, unknown>;
}

export async function facilitatorGet(
  path: string
): Promise<Record<string, unknown>> {
  const url = `${FACILITATOR_URL}${path}`;
  const res = await fetch(url);
  const data = await res.json();

  if (!res.ok) {
    const msg = (data as any).message ?? JSON.stringify(data);
    throw new Error(`[${res.status}] ${path}: ${msg}`);
  }

  return data as Record<string, unknown>;
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function log(bot: string, msg: string): void {
  const ts = new Date().toISOString().slice(11, 19);
  console.log(`[${ts}] [${bot}] ${msg}`);
}

// ERC-20 mint ABI (MockmUSD has public mint)
export const MOCK_MUSD_ABI = [
  {
    name: "mint",
    type: "function",
    stateMutability: "nonpayable",
    inputs: [
      { name: "to", type: "address" },
      { name: "amount", type: "uint256" },
    ],
    outputs: [],
  },
  {
    name: "balanceOf",
    type: "function",
    stateMutability: "view",
    inputs: [{ name: "account", type: "address" }],
    outputs: [{ name: "", type: "uint256" }],
  },
  {
    name: "approve",
    type: "function",
    stateMutability: "nonpayable",
    inputs: [
      { name: "spender", type: "address" },
      { name: "amount", type: "uint256" },
    ],
    outputs: [{ name: "", type: "bool" }],
  },
] as const;
