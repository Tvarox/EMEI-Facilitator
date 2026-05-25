/**
 * signal-bot.ts — Issues invoices on a schedule, forever.
 *
 * Every INTERVAL_SECONDS:
 * 1. Creates an invoice via facilitator API
 * 2. Waits for tx receipt on-chain
 * 3. Extracts invoice ID from the InvoiceCreated event log
 * 4. Presents the invoice
 * 5. Sleeps
 *
 * No probing, no guessing — deterministic invoice ID from tx receipt logs.
 */
import { parseEther } from "viem";
import { SIGNAL_BOT_PK, MOCK_MUSD_ADDR, signalBotAccount, traderBotAccount, publicClient, facilitatorPost, sleep, log, INTERVAL_SECONDS, } from "./shared.js";
// InvoiceCreated event signature: InvoiceCreated(uint256 indexed invoiceId, address indexed issuer, address indexed payer, uint256 amount)
const INVOICE_CREATED_TOPIC = "0x" + "0".repeat(64); // We'll match by topic count instead
/**
 * Extract invoice ID from transaction receipt logs.
 * InvoiceCreated has 4 topics: sig, invoiceId, issuer, payer
 */
function extractInvoiceId(logs) {
    for (const l of logs) {
        const topics = l.topics ?? [];
        // InvoiceCreated: 4 topics (event sig + 3 indexed params)
        if (topics.length === 4) {
            // invoiceId is topic[1] — a uint256 encoded as 32-byte hex
            const idHex = topics[1];
            const id = parseInt(idHex, 16);
            if (id > 0 && id < 1_000_000) {
                return id;
            }
        }
    }
    return null;
}
async function issueInvoice() {
    const epoch = Math.floor(Date.now() / 1000);
    log("signal-bot", `Issuing invoice (1.00 mUSD)...`);
    const createResult = await facilitatorPost("/emei/invoice", {
        payer: traderBotAccount.address,
        amount: parseEther("1").toString(),
        asset: MOCK_MUSD_ADDR,
        line_items: [
            {
                description: `BTC/ETH momentum signal — epoch ${epoch}`,
                amount: parseEther("1").toString(),
                category: "data-signal",
            },
        ],
        terms: {
            term_type: "due_on_receipt",
        },
        collection_mode: "mandate",
    }, SIGNAL_BOT_PK);
    const txHash = createResult.tx_hash;
    log("signal-bot", `  tx=${txHash}`);
    // Wait for on-chain confirmation via receipt
    log("signal-bot", `  Waiting for receipt...`);
    let receipt;
    try {
        receipt = await publicClient.waitForTransactionReceipt({
            hash: txHash,
            timeout: 60_000, // 60s max
        });
    }
    catch (e) {
        log("signal-bot", `  Receipt timeout: ${e.message}. Skipping.`);
        return;
    }
    if (receipt.status === "reverted") {
        log("signal-bot", `  Tx reverted! Skipping.`);
        return;
    }
    // Extract invoice ID from logs
    const invoiceId = extractInvoiceId(receipt.logs);
    if (!invoiceId) {
        log("signal-bot", `  Could not extract invoice ID from ${receipt.logs.length} logs. Skipping.`);
        return;
    }
    log("signal-bot", `  Confirmed: invoice #${invoiceId} (block ${receipt.blockNumber})`);
    // Present the invoice
    log("signal-bot", `  Presenting #${invoiceId}...`);
    try {
        const presentResult = await facilitatorPost("/emei/present", { invoice_id: invoiceId }, SIGNAL_BOT_PK);
        log("signal-bot", `  Presented! tx=${presentResult.tx_hash}`);
    }
    catch (e) {
        log("signal-bot", `  Present failed: ${e.message}`);
    }
    log("signal-bot", `  Complete: #${invoiceId} → ${traderBotAccount.address.slice(0, 10)}... (awaiting collection)`);
}
async function main() {
    log("signal-bot", `Address: ${signalBotAccount.address}`);
    log("signal-bot", `Payer (trader-bot): ${traderBotAccount.address}`);
    log("signal-bot", `Interval: ${INTERVAL_SECONDS}s`);
    log("signal-bot", `Asset: ${MOCK_MUSD_ADDR}`);
    log("signal-bot", "Starting invoice loop...");
    await sleep(3000);
    while (true) {
        try {
            await issueInvoice();
        }
        catch (e) {
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
