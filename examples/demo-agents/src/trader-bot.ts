/**
 * trader-bot.ts — Creates a spending mandate, then exits.
 *
 * The Facilitator's Auto-Collector handles payment from here.
 * Run once at boot after seed.ts completes.
 */

import {
  TRADER_BOT_PK,
  SIGNAL_BOT_PK,
  signalBotAccount,
  traderBotAccount,
  facilitatorPost,
  sleep,
  log,
  MOCK_MUSD_ADDR,
} from "./shared.js";
import { parseEther } from "viem";

async function main() {
  log("trader-bot", `Address: ${traderBotAccount.address}`);
  log("trader-bot", `Counterparty (signal-bot): ${signalBotAccount.address}`);

  // Wait for facilitator readiness
  log("trader-bot", "Waiting 3s for facilitator...");
  await sleep(3000);

  // Create mandate: authorize signal-bot to collect up to 1000 mUSD
  // for category "data-signal", valid for 30 days
  const now = Math.floor(Date.now() / 1000);
  const validFrom = now;
  const validUntil = now + 30 * 24 * 60 * 60; // 30 days

  log("trader-bot", "Creating mandate...");
  log("trader-bot", `  Spend cap: 1000 mUSD`);
  log("trader-bot", `  Counterparties: [${signalBotAccount.address}]`);
  log("trader-bot", `  Categories: [data-signal]`);
  log("trader-bot", `  Valid: ${new Date(validFrom * 1000).toISOString()} → ${new Date(validUntil * 1000).toISOString()}`);

  const result = await facilitatorPost(
    "/emei/mandate",
    {
      spend_cap: parseEther("1000").toString(),
      approved_counterparties: [signalBotAccount.address],
      approved_categories: ["data-signal"],
      valid_from: validFrom,
      valid_until: validUntil,
    },
    TRADER_BOT_PK
  );

  log("trader-bot", `Mandate created! tx: ${(result as any).tx_hash}`);
  log("trader-bot", "Exiting. Auto-Collector will handle payments from here.");
}

main().catch((e) => {
  console.error("[trader-bot] FATAL:", e.message ?? e);
  process.exit(1);
});
