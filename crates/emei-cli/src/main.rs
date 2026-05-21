//! EMEI Skills CLI — agent-facing thin HTTP client for the EMEI backend.
//!
//! All output is structured JSON for machine parsing by agent runtimes.
//! Exit code 0 on success, non-zero on failure.

use clap::{Parser, Subcommand};
use reqwest::Client;
use serde_json::{Value, json};
use std::process;

// ---------------------------------------------------------------------------
// CLI structure
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "emei",
    about = "EMEI Skills CLI — programmable invoicing for agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Wallet operations
    Wallet {
        #[command(subcommand)]
        action: WalletAction,
    },
    /// Invoice operations
    Invoice {
        #[command(subcommand)]
        action: InvoiceAction,
    },
    /// Mandate operations
    Mandate {
        #[command(subcommand)]
        action: MandateAction,
    },
    /// Collect invoice via mandate
    Collect {
        /// Invoice ID
        invoice: u64,
        /// Mandate ID
        mandate: u64,
    },
    /// Check vault balance
    Balance {
        /// Address to check (defaults to own address derived from key)
        address: Option<String>,
    },
    /// Check reputation score
    Reputation {
        /// Address to query
        address: String,
    },
    /// Withdraw from vault
    Withdraw {
        /// Amount in wei
        amount: String,
    },
    /// Pay an invoice (shortcut for `invoice pay`)
    Pay {
        /// Invoice ID
        id: u64,
    },
}

#[derive(Subcommand)]
enum WalletAction {
    /// Register identity (POST /emei/register)
    Create {
        /// Initial reputation score
        #[arg(long, default_value = "100")]
        score: u64,
    },
}

#[derive(Subcommand)]
enum InvoiceAction {
    /// Create a new invoice
    Create {
        /// Payer address
        #[arg(long)]
        payer: String,
        /// Amount (e.g. "0.1", "100", "1.5" — converted to wei using 18 decimals)
        #[arg(long)]
        amount: String,
        /// Asset contract address
        #[arg(long)]
        asset: String,
        /// Line item category
        #[arg(long, default_value = "services")]
        category: String,
        /// Line item description
        #[arg(long, default_value = "Service")]
        description: String,
        /// Terms type: due_on_receipt, net_n_days, milestones
        #[arg(long, default_value = "due_on_receipt")]
        terms: String,
        /// Net days (for net_n_days terms)
        #[arg(long, default_value = "7")]
        net_days: u64,
        /// Collection mode: mandate or pay_link
        #[arg(long, default_value = "pay_link")]
        mode: String,
    },
    /// Present an invoice to the payer
    Present {
        /// Invoice ID
        id: u64,
    },
    /// Pay an invoice
    Pay {
        /// Invoice ID
        id: u64,
    },
    /// List invoices (fetches IDs 1..10)
    List {
        /// Start ID
        #[arg(long, default_value = "1")]
        from: u64,
        /// End ID (inclusive)
        #[arg(long, default_value = "10")]
        to: u64,
    },
    /// Get invoice details
    Get {
        /// Invoice ID
        id: u64,
    },
}

#[derive(Subcommand)]
enum MandateAction {
    /// Create a new mandate
    Create {
        /// Spend cap (e.g. "100", "0.5" — converted to wei using 18 decimals)
        #[arg(long)]
        spend_cap: String,
        /// Approved counterparties (comma-separated addresses)
        #[arg(long)]
        counterparties: String,
        /// Approved categories (comma-separated)
        #[arg(long, default_value = "services")]
        categories: String,
        /// Valid from (unix timestamp)
        #[arg(long)]
        valid_from: u64,
        /// Valid until (unix timestamp)
        #[arg(long)]
        valid_until: u64,
    },
    /// Revoke a mandate
    Revoke {
        /// Mandate ID
        id: u64,
    },
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

struct Config {
    api_url: String,
    private_key: Option<String>,
}

impl Config {
    fn from_env() -> Self {
        Self {
            api_url: std::env::var("EMEI_API_URL")
                .unwrap_or_else(|_| "http://localhost:8080".to_string()),
            private_key: std::env::var("EMEI_PRIVATE_KEY").ok(),
        }
    }
}

// ---------------------------------------------------------------------------
// Amount conversion
// ---------------------------------------------------------------------------

/// Convert a human-readable amount (e.g. "0.1", "100", "1.5") to wei string (18 decimals).
/// If the input is already a large integer (no decimal point and > 1e15), pass it through as-is.
fn to_wei(amount: &str) -> String {
    // If it looks like it's already in wei (large number, no decimal), pass through
    if !amount.contains('.') {
        if let Ok(val) = amount.parse::<u128>() {
            if val > 1_000_000_000_000_000 {
                // Already in wei (> 0.001 token)
                return amount.to_string();
            }
            // Small integer like "100" means 100 tokens
            return format!("{}000000000000000000", val);
        }
        return amount.to_string();
    }

    // Has decimal point — parse and convert
    let parts: Vec<&str> = amount.split('.').collect();
    let whole = parts[0];
    let frac = if parts.len() > 1 { parts[1] } else { "" };

    // Pad or truncate fractional part to 18 digits
    let frac_padded = format!("{:0<18}", frac);
    let frac_18 = &frac_padded[..18];

    // Combine: whole + frac (remove leading zeros from result)
    let wei_str = format!("{}{}", whole, frac_18);
    // Remove leading zeros but keep at least "0"
    let trimmed = wei_str.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

fn build_client(config: &Config) -> Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    if let Some(ref key) = config.private_key {
        headers.insert("X-Private-Key", key.parse().unwrap());
    }
    Client::builder()
        .default_headers(headers)
        .build()
        .expect("failed to build HTTP client")
}

async fn post(client: &Client, url: &str, body: Value) -> Result<Value, String> {
    let resp = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(json!({}));

    if status.is_success() {
        Ok(body)
    } else {
        let msg = body["message"].as_str().unwrap_or("unknown error");
        let code = body["error_code"].as_str().unwrap_or("API_ERROR");
        Err(format!("{code}: {msg}"))
    }
}

async fn get(client: &Client, url: &str) -> Result<Value, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(json!({}));

    if status.is_success() {
        Ok(body)
    } else {
        let msg = body["message"].as_str().unwrap_or("unknown error");
        let code = body["error_code"].as_str().unwrap_or("API_ERROR");
        Err(format!("{code}: {msg}"))
    }
}

async fn delete(client: &Client, url: &str) -> Result<Value, String> {
    let resp = client
        .delete(url)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(json!({}));

    if status.is_success() {
        Ok(body)
    } else {
        let msg = body["message"].as_str().unwrap_or("unknown error");
        let code = body["error_code"].as_str().unwrap_or("API_ERROR");
        Err(format!("{code}: {msg}"))
    }
}

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

fn success(data: Value) {
    let output = json!({ "success": true, "data": data });
    println!("{}", serde_json::to_string(&output).unwrap());
}

fn failure(code: &str, message: &str) -> ! {
    let output = json!({ "success": false, "error": { "code": code, "message": message } });
    println!("{}", serde_json::to_string(&output).unwrap());
    process::exit(1);
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config = Config::from_env();
    let client = build_client(&config);
    let base = &config.api_url;

    let result = match cli.command {
        Commands::Wallet { action } => match action {
            WalletAction::Create { score } => {
                post(
                    &client,
                    &format!("{base}/emei/register"),
                    json!({ "initial_score": score }),
                )
                .await
            }
        },

        Commands::Invoice { action } => match action {
            InvoiceAction::Create {
                payer,
                amount,
                asset,
                category,
                description,
                terms,
                net_days,
                mode,
            } => {
                let amount_wei = to_wei(&amount);
                let terms_obj = match terms.as_str() {
                    "net_n_days" => json!({ "term_type": "net_n_days", "net_days": net_days }),
                    "milestones" => json!({ "term_type": "milestones", "milestones": [] }),
                    _ => json!({ "term_type": "due_on_receipt" }),
                };
                let body = json!({
                    "payer": payer,
                    "amount": amount_wei,
                    "asset": asset,
                    "line_items": [{ "description": description, "amount": amount_wei, "category": category }],
                    "terms": terms_obj,
                    "collection_mode": mode,
                });
                post(&client, &format!("{base}/emei/invoice"), body).await
            }
            InvoiceAction::Present { id } => {
                post(
                    &client,
                    &format!("{base}/emei/present"),
                    json!({ "invoice_id": id }),
                )
                .await
            }
            InvoiceAction::Pay { id } => {
                post(
                    &client,
                    &format!("{base}/emei/pay"),
                    json!({ "invoice_id": id }),
                )
                .await
            }
            InvoiceAction::List { from, to } => {
                let mut invoices = Vec::new();
                for id in from..=to {
                    match get(&client, &format!("{base}/emei/invoice/{id}")).await {
                        Ok(inv) => invoices.push(inv),
                        Err(_) => {} // skip missing
                    }
                }
                Ok(json!(invoices))
            }
            InvoiceAction::Get { id } => get(&client, &format!("{base}/emei/invoice/{id}")).await,
        },

        Commands::Mandate { action } => match action {
            MandateAction::Create {
                spend_cap,
                counterparties,
                categories,
                valid_from,
                valid_until,
            } => {
                let cps: Vec<&str> = counterparties.split(',').map(|s| s.trim()).collect();
                let cats: Vec<&str> = categories.split(',').map(|s| s.trim()).collect();
                let body = json!({
                    "spend_cap": to_wei(&spend_cap),
                    "approved_counterparties": cps,
                    "approved_categories": cats,
                    "valid_from": valid_from,
                    "valid_until": valid_until,
                });
                post(&client, &format!("{base}/emei/mandate"), body).await
            }
            MandateAction::Revoke { id } => {
                delete(&client, &format!("{base}/emei/mandate/{id}")).await
            }
        },

        Commands::Collect { invoice, mandate } => {
            post(
                &client,
                &format!("{base}/emei/collect"),
                json!({ "invoice_id": invoice, "mandate_id": mandate }),
            )
            .await
        }

        Commands::Balance { address } => {
            let addr = address.unwrap_or_else(|| "0x0".to_string());
            get(&client, &format!("{base}/emei/balance/{addr}")).await
        }

        Commands::Reputation { address } => {
            get(&client, &format!("{base}/emei/reputation/{address}")).await
        }

        Commands::Withdraw { amount } => {
            post(
                &client,
                &format!("{base}/emei/withdraw"),
                json!({ "amount": to_wei(&amount) }),
            )
            .await
        }

        Commands::Pay { id } => {
            post(
                &client,
                &format!("{base}/emei/pay"),
                json!({ "invoice_id": id }),
            )
            .await
        }
    };

    match result {
        Ok(data) => success(data),
        Err(e) => failure("API_ERROR", &e),
    }
}
