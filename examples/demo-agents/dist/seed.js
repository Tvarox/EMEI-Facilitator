/**
 * seed.ts — One-shot funding and registration script for all agents.
 *
 * 1. Mints mUSD to payers (trader-bot, research-bot)
 * 2. Registers all 5 identities in ERC-8004
 * 3. Approves Settlement contract for payers
 * 4. Creates mandates:
 *    - trader-bot → signal-bot: 1000 mUSD, category "data-signal"
 *    - trader-bot → compute-bot: 30 mUSD, category "compute" (will exhaust!)
 *    - research-bot → analytics-bot: 500 mUSD, category "analytics"
 */
import "dotenv/config";
import { parseEther, formatEther } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { MOCK_MUSD_ADDR, MOCK_MUSD_ABI, SIGNAL_BOT_PK, TRADER_BOT_PK, signalBotAccount, traderBotAccount, publicClient, walletClient, facilitatorPost, sleep, log, env, } from "./shared.js";
const SETTLEMENT_ADDR = env("EMEI_SETTLEMENT_ADDRESS", "0xfdCb7bA077069A7Da44711Ee6bdB49174AFA4dD0");
// New agents
const COMPUTE_BOT_PK = env("COMPUTE_BOT_PK", "");
const ANALYTICS_BOT_PK = env("ANALYTICS_BOT_PK", "");
const RESEARCH_BOT_PK = env("RESEARCH_BOT_PK", "");
const computeBot = COMPUTE_BOT_PK ? privateKeyToAccount(COMPUTE_BOT_PK) : null;
const analyticsBot = ANALYTICS_BOT_PK ? privateKeyToAccount(ANALYTICS_BOT_PK) : null;
const researchBot = RESEARCH_BOT_PK ? privateKeyToAccount(RESEARCH_BOT_PK) : null;
async function mintAndWait(pk, to, amount, label) {
    const wallet = walletClient(pk);
    log("seed", `Minting ${formatEther(amount)} mUSD to ${label}...`);
    const tx = await wallet.writeContract({
        address: MOCK_MUSD_ADDR,
        abi: MOCK_MUSD_ABI,
        functionName: "mint",
        args: [to, amount],
    });
    log("seed", `  tx: ${tx}`);
    await publicClient.waitForTransactionReceipt({ hash: tx });
}
async function approveAndWait(pk, label) {
    const wallet = walletClient(pk);
    log("seed", `Approving Settlement for ${label}...`);
    const tx = await wallet.writeContract({
        address: MOCK_MUSD_ADDR,
        abi: MOCK_MUSD_ABI,
        functionName: "approve",
        args: [SETTLEMENT_ADDR, parseEther("1000000")],
    });
    log("seed", `  tx: ${tx}`);
    await publicClient.waitForTransactionReceipt({ hash: tx });
}
async function registerAgent(pk, label) {
    log("seed", `Registering ${label} (score=500)...`);
    try {
        const result = await facilitatorPost("/emei/register", { initial_score: 500 }, pk);
        log("seed", `  tx: ${result.tx_hash}`);
    }
    catch (e) {
        if (e.message.includes("AlreadyRegistered")) {
            log("seed", `  Already registered, skipping.`);
        }
        else {
            throw e;
        }
    }
}
async function createMandate(payerPk, payerLabel, counterparty, cap, categories) {
    const now = Math.floor(Date.now() / 1000);
    log("seed", `Creating mandate: ${payerLabel} → ${counterparty.slice(0, 10)}... (cap: ${cap} mUSD, categories: ${categories.join(",")})`);
    const result = await facilitatorPost("/emei/mandate", {
        spend_cap: parseEther(cap).toString(),
        approved_counterparties: [counterparty],
        approved_categories: categories,
        valid_from: now,
        valid_until: now + 30 * 24 * 60 * 60, // 30 days
    }, payerPk);
    log("seed", `  tx: ${result.tx_hash}`);
}
async function main() {
    log("seed", "=== EMEI Multi-Agent Seed ===");
    log("seed", `Signal bot:    ${signalBotAccount.address}`);
    log("seed", `Trader bot:    ${traderBotAccount.address}`);
    if (computeBot)
        log("seed", `Compute bot:   ${computeBot.address}`);
    if (analyticsBot)
        log("seed", `Analytics bot: ${analyticsBot.address}`);
    if (researchBot)
        log("seed", `Research bot:  ${researchBot.address}`);
    log("seed", "");
    // 1. Mint mUSD to payers
    await mintAndWait(TRADER_BOT_PK, traderBotAccount.address, parseEther("2000"), "trader-bot");
    if (researchBot) {
        await mintAndWait(RESEARCH_BOT_PK, researchBot.address, parseEther("1000"), "research-bot");
    }
    // 2. Approve Settlement for payers
    await approveAndWait(TRADER_BOT_PK, "trader-bot");
    if (researchBot) {
        await approveAndWait(RESEARCH_BOT_PK, "research-bot");
    }
    // 3. Register all agents
    await registerAgent(SIGNAL_BOT_PK, "signal-bot");
    await registerAgent(TRADER_BOT_PK, "trader-bot");
    if (computeBot)
        await registerAgent(COMPUTE_BOT_PK, "compute-bot");
    if (analyticsBot)
        await registerAgent(ANALYTICS_BOT_PK, "analytics-bot");
    if (researchBot)
        await registerAgent(RESEARCH_BOT_PK, "research-bot");
    // 4. Create mandates (with waits between same-sender txs)
    // trader-bot → signal-bot: 1000 mUSD for data-signal (happy path)
    await createMandate(TRADER_BOT_PK, "trader-bot", signalBotAccount.address, "1000", ["data-signal"]);
    log("seed", "  Waiting for tx confirmation...");
    await sleep(12000);
    // trader-bot → compute-bot: 30 mUSD for compute (will exhaust after 6 invoices!)
    if (computeBot) {
        await createMandate(TRADER_BOT_PK, "trader-bot", computeBot.address, "30", ["compute"]);
        log("seed", "  Waiting for tx confirmation...");
        await sleep(12000);
    }
    // research-bot → analytics-bot: 500 mUSD for analytics (happy path)
    if (analyticsBot && researchBot) {
        await createMandate(RESEARCH_BOT_PK, "research-bot", analyticsBot.address, "500", ["analytics"]);
    }
    log("seed", "");
    log("seed", "=== Seed complete! ===");
    log("seed", "Mandates created:");
    log("seed", "  trader-bot → signal-bot:  1000 mUSD (data-signal) ✅");
    if (computeBot)
        log("seed", "  trader-bot → compute-bot: 30 mUSD (compute) ⚠️ will exhaust!");
    if (researchBot)
        log("seed", "  research-bot → analytics-bot: 500 mUSD (analytics) ✅");
}
main().catch((e) => {
    console.error("[seed] FATAL:", e.message ?? e);
    process.exit(1);
});
