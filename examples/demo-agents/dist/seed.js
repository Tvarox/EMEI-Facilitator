/**
 * seed.ts — One-shot funding and registration script.
 *
 * 1. Mints mUSD to trader-bot (funds the mandate)
 * 2. Mints a small amount to signal-bot (not strictly needed)
 * 3. Registers both identities in ERC-8004 with score=500
 * 4. Approves the Settlement contract to spend trader-bot's mUSD
 */
import { parseEther, formatEther } from "viem";
import { MOCK_MUSD_ADDR, MOCK_MUSD_ABI, SIGNAL_BOT_PK, TRADER_BOT_PK, signalBotAccount, traderBotAccount, publicClient, walletClient, facilitatorPost, log, env, } from "./shared.js";
const SETTLEMENT_ADDR = env("EMEI_SETTLEMENT_ADDRESS", "0xfdCb7bA077069A7Da44711Ee6bdB49174AFA4dD0");
async function main() {
    log("seed", `Signal bot: ${signalBotAccount.address}`);
    log("seed", `Trader bot: ${traderBotAccount.address}`);
    log("seed", `mUSD token: ${MOCK_MUSD_ADDR}`);
    const traderWallet = walletClient(TRADER_BOT_PK);
    const signalWallet = walletClient(SIGNAL_BOT_PK);
    // 1. Mint mUSD to trader-bot (1000 tokens)
    log("seed", "Minting 1000 mUSD to trader-bot...");
    const mintTxTrader = await traderWallet.writeContract({
        address: MOCK_MUSD_ADDR,
        abi: MOCK_MUSD_ABI,
        functionName: "mint",
        args: [traderBotAccount.address, parseEther("1000")],
    });
    log("seed", `  tx: ${mintTxTrader}`);
    log("seed", "  Waiting for confirmation...");
    await publicClient.waitForTransactionReceipt({ hash: mintTxTrader });
    // 2. Mint mUSD to signal-bot (100 tokens — for receiving payments)
    log("seed", "Minting 100 mUSD to signal-bot...");
    const mintTxSignal = await signalWallet.writeContract({
        address: MOCK_MUSD_ADDR,
        abi: MOCK_MUSD_ABI,
        functionName: "mint",
        args: [signalBotAccount.address, parseEther("100")],
    });
    log("seed", `  tx: ${mintTxSignal}`);
    log("seed", "  Waiting for confirmation...");
    await publicClient.waitForTransactionReceipt({ hash: mintTxSignal });
    // 3. Approve Settlement contract to spend trader-bot's mUSD
    log("seed", "Approving Settlement to spend trader-bot's mUSD...");
    const approveTx = await traderWallet.writeContract({
        address: MOCK_MUSD_ADDR,
        abi: MOCK_MUSD_ABI,
        functionName: "approve",
        args: [SETTLEMENT_ADDR, parseEther("1000000")], // large allowance
    });
    log("seed", `  tx: ${approveTx}`);
    log("seed", "  Waiting for confirmation...");
    await publicClient.waitForTransactionReceipt({ hash: approveTx });
    // 4. Register both identities
    log("seed", "Registering signal-bot identity (score=500)...");
    try {
        const regSignal = await facilitatorPost("/emei/register", { initial_score: 500 }, SIGNAL_BOT_PK);
        log("seed", `  tx: ${regSignal.tx_hash}`);
    }
    catch (e) {
        if (e.message.includes("AlreadyRegistered")) {
            log("seed", "  Already registered, skipping.");
        }
        else {
            throw e;
        }
    }
    log("seed", "Registering trader-bot identity (score=500)...");
    try {
        const regTrader = await facilitatorPost("/emei/register", { initial_score: 500 }, TRADER_BOT_PK);
        log("seed", `  tx: ${regTrader.tx_hash}`);
    }
    catch (e) {
        if (e.message.includes("AlreadyRegistered")) {
            log("seed", "  Already registered, skipping.");
        }
        else {
            throw e;
        }
    }
    // 5. Print balances
    const traderBal = await publicClient.readContract({
        address: MOCK_MUSD_ADDR,
        abi: MOCK_MUSD_ABI,
        functionName: "balanceOf",
        args: [traderBotAccount.address],
    });
    const signalBal = await publicClient.readContract({
        address: MOCK_MUSD_ADDR,
        abi: MOCK_MUSD_ABI,
        functionName: "balanceOf",
        args: [signalBotAccount.address],
    });
    log("seed", `Trader-bot mUSD balance: ${formatEther(traderBal)}`);
    log("seed", `Signal-bot mUSD balance: ${formatEther(signalBal)}`);
    log("seed", "Done! Bots are funded and registered.");
}
main().catch((e) => {
    console.error("[seed] FATAL:", e.message ?? e);
    process.exit(1);
});
