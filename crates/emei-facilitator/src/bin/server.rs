//! EMEI Facilitator standalone server binary.
//!
//! Loads configuration from environment variables, initializes the chain client,
//! SQLite store, and application state, then starts background services and
//! the Axum HTTP server on 0.0.0.0:8080.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};

use emei_facilitator::chain::AlloyChainClient;
use emei_facilitator::config::EmeiConfig;
use emei_facilitator::db::StatementStore;
use emei_facilitator::state::{AppState, ReceiptQueue};
use emei_facilitator::{emei_router, start_services};

#[tokio::main]
async fn main() {
    // Load .env file (from current working directory)
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "emei_facilitator=info,tower_http=info".into()),
        )
        .init();

    tracing::info!("Starting EMEI Facilitator server...");

    // Load configuration from environment
    let config = EmeiConfig::from_env().unwrap_or_else(|e| {
        eprintln!("Configuration error: {e}");
        std::process::exit(1);
    });

    tracing::info!("RPC URL: {}", config.rpc_url);
    tracing::info!("Invoice contract: {:?}", config.invoice_address);
    tracing::info!("Mandate contract: {:?}", config.mandate_address);
    tracing::info!("Settlement contract: {:?}", config.settlement_address);

    // Create chain client
    let chain = AlloyChainClient::new(&config.rpc_url, config.hot_wallet_key).unwrap_or_else(|e| {
        eprintln!("Failed to create chain client: {e}");
        std::process::exit(1);
    });

    // Open SQLite store
    let db = StatementStore::open(&config.sqlite_path)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Failed to open database: {e}");
            std::process::exit(1);
        });

    // Build application state
    let state = Arc::new(AppState {
        chain: Arc::new(chain),
        db,
        receipt_queue: ReceiptQueue::new(),
        nonce_manager: emei_facilitator::nonce::NonceManager::new(),
        config,
        cancel: CancellationToken::new(),
        started_at: std::time::Instant::now(),
    });

    // Start background services
    let _handles = start_services(Arc::clone(&state));
    tracing::info!("Background services started");

    // Build CORS layer
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers(Any);

    // Build router
    let app = emei_router(Arc::clone(&state)).layer(cors);

    // Bind and serve
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .unwrap_or_else(|e| {
            eprintln!("Failed to bind to 0.0.0.0:8080: {e}");
            std::process::exit(1);
        });

    tracing::info!("EMEI Facilitator listening on 0.0.0.0:8080");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(Arc::clone(&state)))
        .await
        .unwrap_or_else(|e| {
            eprintln!("Server error: {e}");
            std::process::exit(1);
        });
}

async fn shutdown_signal(state: Arc<AppState>) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    ctrl_c.await;
    tracing::info!("Shutdown signal received, stopping services...");
    state.cancel.cancel();
}
