/**
 * compute-bot.ts — Issues compute invoices on a schedule.
 *
 * Issues 5 mUSD every 15 min to trader-bot, category "compute".
 * trader-bot's mandate for compute has a LOW cap (30 mUSD).
 * After 6 invoices, the cap is exhausted → invoices go OVERDUE.
 * This demonstrates the cap exhaustion + overdue penalty scenario.
 */
import "dotenv/config";
import { parseEther } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { facilitatorPost, facilitatorGet, sleep, log, env, } from "./shared.js";
const COMPUTE_BOT_PK = env("COMPUTE_BOT_PK");
const TRADER_BOT_PK = env("TRADER_BOT_PK");
const MOCK_MUSD_ADDR = env("MOCK_MUSD_ADDR");
const INTERVAL = parseInt(env("COMPUTE_INTERVAL_SECONDS", "900")); // 15 min default
const computeBot = privateKeyToAccount(COMPUTE_BOT_PK);
const traderBot = privateKeyToAccount(TRADER_BOT_PK);
let lastKnownId = parseInt(process.env.INVOICE_START_ID ?? "0");
async function discoverNewInvoiceId() {
    const probeIds = [];
    for (let id = lastKnownId + 1; id <= lastKnownId + 5; id++)
        probeIds.push(id);
    for (let id = lastKnownId + 10; id <= lastKnownId + 50; id += 5)
        probeIds.push(id);
    const uniqueIds = [...new Set(probeIds)].sort((a, b) => b - a);
    for (const id of uniqueIds) {
        try {
            const invoice = await facilitatorGet(`/emei/invoice/${id}`);
            const issuer = (invoice.issuer ?? "").toLowerCase();
            const status = invoice.status ?? "";
            if (issuer === computeBot.address.toLowerCase() && status === "ISSUED") {
                return id;
            }
        }
        catch {
            continue;
        }
    }
    return null;
}
async function issueInvoice() {
    const epoch = Math.floor(Date.now() / 1000);
    log("compute-bot", `Issuing invoice (5.00 mUSD)...`);
    const createResult = await facilitatorPost("/emei/invoice", {
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
    }, COMPUTE_BOT_PK);
    const txHash = createResult.tx_hash;
    log("compute-bot", `  Created: tx=${txHash}`);
    log("compute-bot", `  Waiting for confirmation...`);
    await sleep(10000);
    const invoiceId = await discoverNewInvoiceId();
    if (!invoiceId) {
        log("compute-bot", `  Could not discover invoice ID (will retry next cycle)`);
        return;
    }
    lastKnownId = invoiceId;
    log("compute-bot", `  Discovered invoice ID: ${invoiceId}`);
    log("compute-bot", `  Presenting invoice #${invoiceId}...`);
    try {
        const presentResult = await facilitatorPost("/emei/present", { invoice_id: invoiceId }, COMPUTE_BOT_PK);
        log("compute-bot", `  Presented! tx=${presentResult.tx_hash}`);
    }
    catch (e) {
        log("compute-bot", `  Present failed: ${e.message}`);
    }
    log("compute-bot", `  Invoice #${invoiceId} complete (5.00 mUSD → ${traderBot.address.slice(0, 10)}...)`);
}
async function main() {
    log("compute-bot", `Address: ${computeBot.address}`);
    log("compute-bot", `Payer (trader-bot): ${traderBot.address}`);
    log("compute-bot", `Interval: ${INTERVAL}s`);
    log("compute-bot", `Amount: 5.00 mUSD per invoice`);
    log("compute-bot", `Category: compute`);
    log("compute-bot", "Starting invoice loop...");
    await sleep(8000);
    while (true) {
        try {
            await issueInvoice();
        }
        catch (e) {
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
