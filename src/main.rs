// src/main.rs
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use log;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use subxt::ext::sp_core::sr25519::Pair;
use subxt::ext::sp_core::Pair as PairTrait;
use subxt::tx::PairSigner;
use subxt::utils::AccountId32;
use subxt::{OnlineClient, PolkadotConfig};
use tower_http::cors::CorsLayer;

// Include the generated runtime
#[subxt::subxt(runtime_metadata_path = "src/metadata.scale")]
pub mod chain_a {}

#[derive(Debug, Serialize, Deserialize)]
struct DoSomethingRequest {
    value: u32,
    signer: Option<String>, // Optional signer, defaults to Alice
}

#[derive(Debug, Serialize, Deserialize)]
struct DoSomethingResponse {
    success: bool,
    transaction_hash: Option<String>,
    block_hash: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GetStorageResponse {
    value: Option<u32>,
    block_hash: String,
}

#[derive(Clone)]
struct AppState {
    client: OnlineClient<PolkadotConfig>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Connect to Chain A
    let client = OnlineClient::<PolkadotConfig>::from_url("ws://localhost:9944").await?;
    log::info!("Connected to Chain A at ws://localhost:9944");

    let state = AppState { client };

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/do-something", post(do_something_handler))
        .route("/get-storage", get(get_storage_handler))
        .route("/latest-events", get(get_latest_events))
        .layer(CorsLayer::permissive())
        .with_state(state);

    // Start the server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001").await?;
    log::info!("Backend API server running on http://127.0.0.1:3001");

    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> Json<HashMap<String, String>> {
    let mut response = HashMap::new();
    response.insert("status".to_string(), "healthy".to_string());
    response.insert("service".to_string(), "Chain A Backend API".to_string());
    Json(response)
}

async fn do_something_handler(
    State(state): State<AppState>,
    Json(payload): Json<DoSomethingRequest>,
) -> Result<Json<DoSomethingResponse>, StatusCode> {
    log::info!(
        "Received do_something request with value: {}",
        payload.value
    );

    // Parse signer (default to Alice if not provided)
    let signer_seed = payload.signer.unwrap_or_else(|| "//Alice".to_string());
    let signer = match Pair::from_string(&signer_seed, None) {
        Ok(pair) => pair,
        Err(_) => {
            return Ok(Json(DoSomethingResponse {
                success: false,
                transaction_hash: None,
                block_hash: None,
                error: Some("Invalid signer".to_string()),
            }));
        }
    };

    // Get current nonce from chain (more reliable than tracking)
    let account_id = AccountId32::from(signer.public());
    let account_nonce = match get_account_nonce(&state.client, &account_id).await {
        Ok(nonce) => nonce,
        Err(e) => {
            log::error!("Failed to get account nonce: {:?}", e);
            return Ok(Json(DoSomethingResponse {
                success: false,
                transaction_hash: None,
                block_hash: None,
                error: Some("Failed to get account nonce".to_string()),
            }));
        }
    };

    // Create the extrinsic
    let call = chain_a::tx().template().do_something(payload.value);

    // Wrap the pair in a PairSigner
    let pair_signer = PairSigner::new(signer);

    // Create transaction with explicit nonce to avoid conflicts
    // Use create_signed_with_nonce for manual nonce management
    // Submit and watch the transaction
    match state
        .client
        .tx()
        .sign_and_submit_then_watch_default(&call, &pair_signer)
        .await
    {
        Ok(progress) => {
            // Wait for the transaction to be included in a block
            match progress.wait_for_finalized_success().await {
                Ok(events) => {
                    let tx_hash = format!("{:?}", events.extrinsic_hash());

                    // Get the block hash from the latest finalized block
                    let block_hash = match state.client.blocks().at_latest().await {
                        Ok(block) => format!("{:?}", block.hash()),
                        Err(_) => "unknown".to_string(),
                    };

                    log::info!("Transaction successful: {}", tx_hash);

                    Ok(Json(DoSomethingResponse {
                        success: true,
                        transaction_hash: Some(tx_hash),
                        block_hash: Some(block_hash),
                        error: None,
                    }))
                }
                Err(e) => {
                    log::error!("Transaction failed: {:?}", e);
                    Ok(Json(DoSomethingResponse {
                        success: false,
                        transaction_hash: None,
                        block_hash: None,
                        error: Some(format!("Transaction failed: {:?}", e)),
                    }))
                }
            }
        }
        Err(e) => {
            log::error!("Failed to submit transaction: {:?}", e);
            Ok(Json(DoSomethingResponse {
                success: false,
                transaction_hash: None,
                block_hash: None,
                error: Some(format!("Failed to submit: {:?}", e)),
            }))
        }
    }
}

// Helper function to get account nonce
async fn get_account_nonce(
    client: &OnlineClient<PolkadotConfig>,
    account_id: &AccountId32,
) -> Result<u32, Box<dyn std::error::Error>> {
    let account_info = client
        .storage()
        .at_latest()
        .await?
        .fetch(&chain_a::storage().system().account(account_id))
        .await?;

    match account_info {
        Some(info) => Ok(info.nonce),
        None => Ok(0), // New account starts with nonce 0
    }
}

async fn get_storage_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<Json<GetStorageResponse>, StatusCode> {
    // Get the latest finalized block
    let latest_block = match state.client.blocks().at_latest().await {
        Ok(block) => block,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Query the storage
    let storage_query = chain_a::storage().template().something();

    match latest_block.storage().fetch(&storage_query).await {
        Ok(value) => Ok(Json(GetStorageResponse {
            value,
            block_hash: format!("{:?}", latest_block.hash()),
        })),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_latest_events(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<Json<Vec<String>>, StatusCode> {
    // Get latest finalized block
    let latest_block = match state.client.blocks().at_latest().await {
        Ok(block) => block,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Get events from the block
    let events = match latest_block.events().await {
        Ok(events) => events,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let mut event_list = Vec::new();

    // Filter for template pallet events
    for event in events.iter() {
        if let Ok(event) = event {
            if let Ok(template_event) =
                event.as_event::<chain_a::template::events::SomethingStored>()
            {
                if let Some(stored_event) = template_event {
                    event_list.push(format!(
                        "SomethingStored: value={}, who={:?}",
                        stored_event.something, stored_event.who
                    ));
                }
            }
        }
    }

    Ok(Json(event_list))
}
