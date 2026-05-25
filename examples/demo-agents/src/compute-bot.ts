/**
 * compute-bot.ts — Issues compute invoices on a schedule.
 *
 * Issues 5 mUSD every 15 min to trader-bot, category "compute".
 * trader-bot's mandate for compute has a LOW cap (30 mUSD).
 * After 6 invoices, the cap is exhausted → invoices go OVERDUE.
 * This demonstrates the cap exhaustion + overdue penalty scenario.
 */

import "dotenv/config";
import { parseEther, type Hex } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import {
  publicClient,
  facilitatorPost,
  sleep,
  log,
  env,
} from "./shared.js";

const COMPUTE_BOT_PK = env("COMPUTE_BOT_PK") as Hex;
const TRADER_BOT_PK = env("TRADER_BOT_PK") as Hex;
const MOCK_MUSD_ADDR = env("MOCK_MUSD_ADDR") as Hex;
const INTERVAL = parseInt(env("COMPUTE_INTERVAL_SECONDS", "900"));

const computeBot = privateKeyToAccount(COMPUTE_BOT_PK);
const traderBot = privateKeyToAccount(TRADER_BOT_PK);

function extractInvoiceId(logs: any[]): number | null {
  for (const l of logs) {
    const topics = l.topics ?? [];
    if (topics.length === 4) {
      const id = parseInt(topics[1] as string, 16);
      if (id > 0 && id < 1_000_000) return id;
    }
  }
  return null;
}

async function issueInvoice(): Promise<void> {
  const epoch = Math.floor(Date.now() / 1000);

  log("compute-bot", `Issuing invoice (5.00 mUSD)...`);

  const createResult = await facilitatorPost(
    "/emei/invoice",
    {
      payer: traderBot.address,
      amount: parseEther("5").toString(),
      asset: MOCK_MUSD_ADDR,
      line_items: [
        {
          description: `GPU compute batch — epoch ${epoch}`,
          amount: parseEther("5").toString(),
          category: "compute",
        },
      ],
      terms: { term_type: "due_on_receipt" },
      collection_mode: "mandate",
    },
    COMPUTE_BOT_PK
  );

  const txHash = (createResult as any).tx_hash as Hex;
  log("compute-bot", `  tx=${txHash}`);

  log("compute-bot", `  Waiting for receipt...`);
  let receipt;
  try {
    receipt = await publicClient.waitForTransactionReceipt({
      hash: txHash,
      timeout: 60_000,
    });
  } catch (e: any) {
    log("compute-bot", `  Receipt timeout: ${e.message}. Skipping.`);
    return;
  }

  if (receipt.status === "reverted") {
    log("compute-bot", `  Tx reverted! Skipping.`);
    return;
  }

  const invoiceId = extractInvoiceId(receipt.logs);
  if (!invoiceId) {
    log("compute-bot", `  Could not extract invoice ID from logs. Skipping.`);
    return;
  }

  log("compute-bot", `  Confirmed: invoice #${invoiceId} (block ${receipt.blockNumber})`);

  log("compute-bot", `  Presenting #${invoiceId}...`);
  try {
    const presentResult = await facilitatorPost(
      "/emei/present",
      { invoice_id: invoiceId },
      COMPUTE_BOT_PK
    );
    log("compute-bot", `  Presented! tx=${(presentResult as any).tx_hash}`);
  } catch (e: any) {
    log("compute-bot", `  Present failed: ${e.message}`);
  }

  log("compute-bot", `  Complete: #${invoiceId} (5.00 mUSD → ${traderBot.address.slice(0, 10)}...)`);
}

async function main() {
  log("compute-bot", `Address: ${computeBot.address}`);
  log("compute-bot", `Payer (trader-bot): ${traderBot.address}`);
  log("compute-bot", `Interval: ${INTERVAL}s`);
  log("compute-bot", `Amount: 5.00 mUSD | Category: compute`);
  log("compute-bot", "Starting invoice loop...");

  await sleep(8000);

  while (true) {
    try {
      await issueInvoice();
    } catch (e: any) {
      log("compute-bot", `ERROR: ${e.message}`);
    }

    log("compute-bot", `Sleeping ${INTERVAL}s...`);
    await sleep(INTERVAL * 1000);
  }
}

main().catch((e) => {
  console.error("[compute-bot] FATAL:", e.message ?? e);
  process.exit(1);
});
