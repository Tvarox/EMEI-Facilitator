/**
 * analytics-bot.ts — Issues analytics invoices to research-bot.
 *
 * Issues 2 mUSD every 20 min to research-bot, category "analytics".
 * research-bot has a large mandate (500 mUSD) so these always get paid.
 * Demonstrates multi-party billing (separate issuer/payer pair).
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

const ANALYTICS_BOT_PK = env("ANALYTICS_BOT_PK") as Hex;
const RESEARCH_BOT_PK = env("RESEARCH_BOT_PK") as Hex;
const MOCK_MUSD_ADDR = env("MOCK_MUSD_ADDR") as Hex;
const INTERVAL = parseInt(env("ANALYTICS_INTERVAL_SECONDS", "1200"));

const analyticsBot = privateKeyToAccount(ANALYTICS_BOT_PK);
const researchBot = privateKeyToAccount(RESEARCH_BOT_PK);

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

  log("analytics-bot", `Issuing invoice (2.00 mUSD)...`);

  const createResult = await facilitatorPost(
    "/emei/invoice",
    {
      payer: researchBot.address,
      amount: parseEther("2").toString(),
      asset: MOCK_MUSD_ADDR,
      line_items: [
        {
          description: `Market analytics report — epoch ${epoch}`,
          amount: parseEther("2").toString(),
          category: "analytics",
        },
      ],
      terms: { term_type: "due_on_receipt" },
      collection_mode: "mandate",
    },
    ANALYTICS_BOT_PK
  );

  const txHash = (createResult as any).tx_hash as Hex;
  log("analytics-bot", `  tx=${txHash}`);

  log("analytics-bot", `  Waiting for receipt...`);
  let receipt;
  try {
    receipt = await publicClient.waitForTransactionReceipt({
      hash: txHash,
      timeout: 60_000,
    });
  } catch (e: any) {
    log("analytics-bot", `  Receipt timeout: ${e.message}. Skipping.`);
    return;
  }

  if (receipt.status === "reverted") {
    log("analytics-bot", `  Tx reverted! Skipping.`);
    return;
  }

  const invoiceId = extractInvoiceId(receipt.logs);
  if (!invoiceId) {
    log("analytics-bot", `  Could not extract invoice ID from logs. Skipping.`);
    return;
  }

  log("analytics-bot", `  Confirmed: invoice #${invoiceId} (block ${receipt.blockNumber})`);

  log("analytics-bot", `  Presenting #${invoiceId}...`);
  try {
    const presentResult = await facilitatorPost(
      "/emei/present",
      { invoice_id: invoiceId },
      ANALYTICS_BOT_PK
    );
    log("analytics-bot", `  Presented! tx=${(presentResult as any).tx_hash}`);
  } catch (e: any) {
    log("analytics-bot", `  Present failed: ${e.message}`);
  }

  log("analytics-bot", `  Complete: #${invoiceId} (2.00 mUSD → ${researchBot.address.slice(0, 10)}...)`);
}

async function main() {
  log("analytics-bot", `Address: ${analyticsBot.address}`);
  log("analytics-bot", `Payer (research-bot): ${researchBot.address}`);
  log("analytics-bot", `Interval: ${INTERVAL}s`);
  log("analytics-bot", `Amount: 2.00 mUSD | Category: analytics`);
  log("analytics-bot", "Starting invoice loop...");

  await sleep(12000);

  while (true) {
    try {
      await issueInvoice();
    } catch (e: any) {
      log("analytics-bot", `ERROR: ${e.message}`);
    }

    log("analytics-bot", `Sleeping ${INTERVAL}s...`);
    await sleep(INTERVAL * 1000);
  }
}

main().catch((e) => {
  console.error("[analytics-bot] FATAL:", e.message ?? e);
  process.exit(1);
});
