/**
 * analytics-bot.ts — Issues analytics invoices to research-bot.
 *
 * Issues 2 mUSD every 20 min to research-bot, category "analytics".
 * research-bot has a large mandate (500 mUSD) so these always get paid.
 * Demonstrates multi-party billing (separate issuer/payer pair).
 */

import "dotenv/config";
import { parseEther } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import {
  facilitatorPost,
  facilitatorGet,
  sleep,
  log,
  env,
} from "./shared.js";

const ANALYTICS_BOT_PK = env("ANALYTICS_BOT_PK") as `0x${string}`;
const RESEARCH_BOT_PK = env("RESEARCH_BOT_PK") as `0x${string}`;
const MOCK_MUSD_ADDR = env("MOCK_MUSD_ADDR") as `0x${string}`;
const INTERVAL = parseInt(env("ANALYTICS_INTERVAL_SECONDS", "1200")); // 20 min default

const analyticsBot = privateKeyToAccount(ANALYTICS_BOT_PK);
const researchBot = privateKeyToAccount(RESEARCH_BOT_PK);

let lastKnownId = parseInt(process.env.INVOICE_START_ID ?? "0");

async function discoverNewInvoiceId(): Promise<number | null> {
  const probeIds: number[] = [];
  for (let id = lastKnownId + 1; id <= lastKnownId + 5; id++) probeIds.push(id);
  for (let id = lastKnownId + 10; id <= lastKnownId + 50; id += 5) probeIds.push(id);
  const uniqueIds = [...new Set(probeIds)].sort((a, b) => b - a);

  for (const id of uniqueIds) {
    try {
      const invoice = await facilitatorGet(`/emei/invoice/${id}`);
      const issuer = ((invoice as any).issuer ?? "").toLowerCase();
      const status = (invoice as any).status ?? "";
      if (issuer === analyticsBot.address.toLowerCase() && status === "ISSUED") {
        return id;
      }
    } catch {
      continue;
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

  const txHash = (createResult as any).tx_hash;
  log("analytics-bot", `  Created: tx=${txHash}`);

  log("analytics-bot", `  Waiting for confirmation...`);
  await sleep(10000);

  const invoiceId = await discoverNewInvoiceId();
  if (!invoiceId) {
    log("analytics-bot", `  Could not discover invoice ID (will retry next cycle)`);
    return;
  }

  lastKnownId = invoiceId;
  log("analytics-bot", `  Discovered invoice ID: ${invoiceId}`);

  log("analytics-bot", `  Presenting invoice #${invoiceId}...`);
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

  log("analytics-bot", `  Invoice #${invoiceId} complete (2.00 mUSD → ${researchBot.address.slice(0, 10)}...)`);
}

async function main() {
  log("analytics-bot", `Address: ${analyticsBot.address}`);
  log("analytics-bot", `Payer (research-bot): ${researchBot.address}`);
  log("analytics-bot", `Interval: ${INTERVAL}s`);
  log("analytics-bot", `Amount: 2.00 mUSD per invoice`);
  log("analytics-bot", `Category: analytics`);
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
