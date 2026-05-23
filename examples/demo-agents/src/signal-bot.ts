/**
 * signal-bot.ts — Issues invoices on a schedule, forever.
 *
 * Every INTERVAL_SECONDS (default 300 = 5 min):
 * 1. Creates an invoice for 1 mUSD to trader-bot
 * 2. Waits for confirmation, then discovers the invoice ID by probing
 * 3. Presents it
 * 4. Logs and sleeps
 *
 * The Facilitator's Auto-Collector picks up presented invoices
 * that match the trader-bot's mandate and collects them automatically.
 */

import { parseEther } from "viem";
import {
  SIGNAL_BOT_PK,
  MOCK_MUSD_ADDR,
  signalBotAccount,
  traderBotAccount,
  facilitatorPost,
  facilitatorGet,
  sleep,
  log,
  INTERVAL_SECONDS,
} from "./shared.js";

// Track the highest known invoice ID so we can find new ones
let lastKnownId = parseInt(process.env.INVOICE_START_ID ?? "0");

/**
 * Probe invoice IDs to find the one we just created.
 * Strategy: query getInvoiceCount from the facilitator, then check the last few.
 */
async function discoverNewInvoiceId(): Promise<number | null> {
  // First, try to get the total invoice count from the chain
  // by probing IDs from lastKnownId+1 upward AND from the high end
  const probeIds: number[] = [];

  // Probe forward from last known
  for (let id = lastKnownId + 1; id <= lastKnownId + 5; id++) {
    probeIds.push(id);
  }

  // Also probe high IDs (in case we're far behind)
  // Try to find the latest by binary-search-style probing
  for (let id = lastKnownId + 10; id <= lastKnownId + 50; id += 5) {
    probeIds.push(id);
  }

  // Deduplicate and sort descending (check newest first)
  const uniqueIds = [...new Set(probeIds)].sort((a, b) => b - a);

  for (const id of uniqueIds) {
    try {
      const invoice = await facilitatorGet(`/emei/invoice/${id}`);
      const issuer = ((invoice as any).issuer ?? "").toLowerCase();
      const status = (invoice as any).status ?? "";

      // Check if this invoice belongs to us and is in ISSUED state
      if (issuer === signalBotAccount.address.toLowerCase() && status === "ISSUED") {
        return id;
      }
    } catch {
      // Invoice doesn't exist, skip
      continue;
    }
  }
  return null;
}

async function issueInvoice(): Promise<void> {
  const epoch = Math.floor(Date.now() / 1000);

  log("signal-bot", `Issuing invoice (1.00 mUSD)...`);

  // Create invoice — due immediately (due_on_receipt) for demo velocity
  const createResult = await facilitatorPost(
    "/emei/invoice",
    {
      payer: traderBotAccount.address,
      amount: parseEther("1").toString(),
      asset: MOCK_MUSD_ADDR,
      line_items: [
        {
          description: `BTC/ETH momentum signal feed — epoch ${epoch}`,
          amount: parseEther("1").toString(),
          category: "data-signal",
        },
      ],
      terms: {
        term_type: "due_on_receipt",
      },
      collection_mode: "mandate",
    },
    SIGNAL_BOT_PK
  );

  const txHash = (createResult as any).tx_hash;
  log("signal-bot", `  Created: tx=${txHash}`);

  // Wait for the tx to confirm on-chain
  log("signal-bot", `  Waiting for confirmation...`);
  await sleep(10000);

  // Discover the actual invoice ID
  const invoiceId = await discoverNewInvoiceId();

  if (!invoiceId) {
    log("signal-bot", `  Could not discover invoice ID (will retry next cycle)`);
    return;
  }

  lastKnownId = invoiceId;
  log("signal-bot", `  Discovered invoice ID: ${invoiceId}`);

  // Present the invoice
  log("signal-bot", `  Presenting invoice #${invoiceId}...`);
  try {
    const presentResult = await facilitatorPost(
      "/emei/present",
      { invoice_id: invoiceId },
      SIGNAL_BOT_PK
    );
    log("signal-bot", `  Presented! tx=${(presentResult as any).tx_hash}`);
  } catch (e: any) {
    log("signal-bot", `  Present failed: ${e.message}`);
  }

  log(
    "signal-bot",
    `  Invoice #${invoiceId} complete (1.00 mUSD → ${traderBotAccount.address.slice(0, 10)}...)`
  );
}

async function main() {
  log("signal-bot", `Address: ${signalBotAccount.address}`);
  log("signal-bot", `Payer (trader-bot): ${traderBotAccount.address}`);
  log("signal-bot", `Interval: ${INTERVAL_SECONDS}s`);
  log("signal-bot", `Asset: ${MOCK_MUSD_ADDR}`);
  log("signal-bot", `Starting invoice ID probe from: ${lastKnownId}`);
  log("signal-bot", "Starting invoice loop...");

  // Initial delay to let other services start
  await sleep(5000);

  while (true) {
    try {
      await issueInvoice();
    } catch (e: any) {
      log("signal-bot", `ERROR: ${e.message}`);
    }

    log("signal-bot", `Sleeping ${INTERVAL_SECONDS}s...`);
    await sleep(INTERVAL_SECONDS * 1000);
  }
}

main().catch((e) => {
  console.error("[signal-bot] FATAL:", e.message ?? e);
  process.exit(1);
});
