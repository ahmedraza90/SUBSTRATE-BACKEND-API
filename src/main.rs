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
use std::{collections::HashMap, sync::Arc};
use subxt::{
    ext::sp_core::{sr25519::Pair, Pair as PairTrait},
    tx::PairSigner,
    utils::AccountId32,
    OnlineClient, SubstrateConfig,
};
use tokio::sync::Mutex;
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

// Updated AppState to include nonce manager
#[derive(Clone)]
struct AppState {
    client: OnlineClient<SubstrateConfig>,
    nonce_manager: NonceManager,
}

// Production-grade nonce manager
#[derive(Clone)]
pub struct NonceManager {
    // Track nonces per account using the raw bytes as key
    nonce_cache: Arc<Mutex<HashMap<[u8; 32], u64>>>,
    // Keep reference to client for chain queries
    client: OnlineClient<SubstrateConfig>,
}

impl NonceManager {
    pub fn new(client: OnlineClient<SubstrateConfig>) -> Self {
        Self {
            nonce_cache: Arc::new(Mutex::new(HashMap::new())),
            client,
        }
    }

    /// Get the next nonce for an account, handling synchronization
    pub async fn get_next_nonce(
        &self,
        account_id: &AccountId32,
    ) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        let mut cache = self.nonce_cache.lock().await;

        // Get the account's current nonce from our cache
        let account_key = account_id.0;
        let cached_nonce = cache.get(&account_key).copied();

        // Get the chain's current nonce
        let chain_nonce = self.client.tx().account_nonce(account_id).await?;

        // Determine which nonce to use
        let nonce_to_use = match cached_nonce {
            Some(cached) => {
                // Use whichever is higher: our cached nonce or chain nonce
                cached.max(chain_nonce)
            }
            None => {
                // First time seeing this account, use chain nonce
                chain_nonce
            }
        };

        // Reserve the next nonce for future transactions
        cache.insert(account_key, nonce_to_use + 1);

        log::info!(
            "🔢 Account {:?}: chain_nonce={}, cached_nonce={:?}, using_nonce={}",
            account_id,
            chain_nonce,
            cached_nonce,
            nonce_to_use
        );

        Ok(nonce_to_use)
    }

    /// Reset nonce cache for an account (useful if transaction fails)
    pub async fn reset_nonce(&self, account_id: &AccountId32, failed_nonce: u64) {
        let mut cache = self.nonce_cache.lock().await;

        // Reset to the failed nonce so it can be reused
        cache.insert(account_id.0, failed_nonce);

        log::warn!(
            "🔄 Reset nonce for account {:?} to {}",
            account_id,
            failed_nonce
        );
    }

    /// Sync with chain (call periodically to stay in sync)
    pub async fn sync_with_chain(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut cache = self.nonce_cache.lock().await;
        // For each account in our cache, check if we're out of sync
        // For each account in our cache, check if we're out of sync
        for (account_key, cached_nonce) in cache.iter_mut() {
            let account_id = AccountId32(*account_key);
            let chain_nonce = self.client.tx().account_nonce(&account_id).await?;

            // If chain is ahead, update our cache
            if chain_nonce > *cached_nonce {
                log::info!(
                    "📊 Syncing account {:?}: {} -> {}",
                    account_id,
                    *cached_nonce,
                    chain_nonce
                );
                *cached_nonce = chain_nonce;
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Connect to Chain A
    let client = OnlineClient::<SubstrateConfig>::from_url("ws://localhost:9944").await?;
    log::info!("Connected to Chain A at ws://localhost:9944");

    // Create nonce manager
    let nonce_manager = NonceManager::new(client.clone());

    // Start background sync task
    let sync_manager = nonce_manager.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            if let Err(e) = sync_manager.sync_with_chain().await {
                log::error!("🔄 Nonce sync failed: {:?}", e);
            }
        }
    });

    let state = AppState {
        client,
        nonce_manager,
    };

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
    let account_id = AccountId32::from(signer.public());

    // 🎯 Get next nonce using our production nonce manager
    let nonce = match state.nonce_manager.get_next_nonce(&account_id).await {
        Ok(nonce) => nonce,
        Err(e) => {
            log::error!("❌ Failed to get nonce: {:?}", e);
            return Ok(Json(DoSomethingResponse {
                success: false,
                transaction_hash: None,
                block_hash: None,
                error: Some(format!("Failed to get nonce: {:?}", e)),
            }));
        }
    };

    // Create the extrinsic
    let call = chain_a::tx().template().do_something(payload.value);

    // Wrap the pair in a PairSigner
    let pair_signer = PairSigner::new(signer);

    // 🔥 THE FIX: Try different approaches based on what's available
    let signed_tx =
        match create_signed_transaction_with_nonce(&state.client, &call, &pair_signer, nonce).await
        {
            Ok(tx) => tx,
            Err(e) => {
                // Reset nonce since we failed to create transaction
                state.nonce_manager.reset_nonce(&account_id, nonce).await;

                log::error!("❌ Failed to create signed transaction: {:?}", e);
                return Ok(Json(DoSomethingResponse {
                    success: false,
                    transaction_hash: None,
                    block_hash: None,
                    error: Some(format!("Failed to create transaction: {:?}", e)),
                }));
            }
        };

    // Submit transaction
    match signed_tx.submit_and_watch().await {
        Ok(progress) => {
            match progress.wait_for_finalized_success().await {
                Ok(events) => {
                    let tx_hash = format!("{:?}", events.extrinsic_hash());

                    // Get the block hash from the latest finalized block
                    let block_hash = match state.client.blocks().at_latest().await {
                        Ok(block) => format!("{:?}", block.hash()),
                        Err(_) => "unknown".to_string(),
                    };

                    log::info!(
                        "✅ Transaction successful: {} in block {} (nonce: {})",
                        tx_hash,
                        block_hash,
                        nonce
                    );

                    Ok(Json(DoSomethingResponse {
                        success: true,
                        transaction_hash: Some(tx_hash),
                        block_hash: Some(block_hash),
                        error: None,
                    }))
                }

                Err(e) => {
                    // Reset nonce for retry
                    state.nonce_manager.reset_nonce(&account_id, nonce).await;

                    log::error!("❌ Transaction failed during finalization: {:?}", e);
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
            // Reset nonce for retry
            state.nonce_manager.reset_nonce(&account_id, nonce).await;

            log::error!("❌ Failed to submit transaction: {:?}", e);
            Ok(Json(DoSomethingResponse {
                success: false,
                transaction_hash: None,
                block_hash: None,
                error: Some(format!("Failed to submit: {:?}", e)),
            }))
        }
    }
    // Create transaction with explicit nonce to avoid conflicts
    // Use create_signed_with_nonce for manual nonce management
    // Submit and watch the transaction
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

// ADD this helper function at the end of your file:
async fn create_signed_transaction_with_nonce<Call>(
    client: &OnlineClient<SubstrateConfig>,
    call: &Call,
    signer: &PairSigner<SubstrateConfig, Pair>,
    _nonce: u64, // We'll try to use this but have fallbacks
) -> Result<
    subxt::tx::SubmittableExtrinsic<SubstrateConfig, OnlineClient<SubstrateConfig>>,
    subxt::Error,
>
where
    Call: subxt::tx::Payload,
{
    log::info!("🔧 Attempting to create signed transaction...");

    // Method 1: Try the original approach (will likely fail but let's try)
    if let Ok(tx) = try_with_explicit_nonce(client, call, signer, _nonce).await {
        log::info!("✅ Used explicit nonce method");
        return Ok(tx);
    }

    // Method 2: Fallback to default signing (subxt handles nonce internally)
    log::warn!("⚠️ Explicit nonce failed, using fallback method");
    match client
        .tx()
        .create_signed(call, signer, Default::default())
        .await
    {
        Ok(tx) => {
            log::info!("✅ Used fallback signing method");
            Ok(tx)
        }
        Err(e) => {
            log::error!("❌ All signing methods failed: {:?}", e);
            Err(e)
        }
    }
}

// This will probably fail, but we try it first
async fn try_with_explicit_nonce<Call>(
    client: &OnlineClient<SubstrateConfig>,
    call: &Call,
    signer: &PairSigner<SubstrateConfig, Pair>,
    nonce: u64,
) -> Result<
    subxt::tx::SubmittableExtrinsic<SubstrateConfig, OnlineClient<SubstrateConfig>>,
    subxt::Error,
>
where
    Call: subxt::tx::Payload,
{
    use subxt::config::DefaultExtrinsicParamsBuilder;
    let params = DefaultExtrinsicParamsBuilder::<SubstrateConfig>::new()
        .nonce(nonce)
        .build();
    return client.tx().create_signed_offline(call, signer, params);
}

// DefaultExtrinsicParams - This is like a "settings package" that contains all the configuration needed for a blockchain transaction (like fees, nonce, mortality, etc.)
// DefaultExtrinsicParamsBuilder - This is like a "settings builder" that lets you create and customize those settings step by step.

//configuration needed for a blockchain transaction
// pub struct DefaultExtrinsicParamsBuilder<T: Config> {
//     /// `None` means the tx will be immortal.
//     mortality: Option<Mortality<T::Hash>>,
//     /// `None` means the nonce will be automatically set.
//     nonce: Option<u64>,
//     /// `None` means we'll use the native token.
//     tip_of_asset_id: Option<T::AssetId>,
//     tip: u128,
//     tip_of: u128,
// }

// struct Mortality<Hash> {
//     /// Block hash that mortality starts from
//     checkpoint_hash: Hash,
//     /// Block number that mortality starts from (must
//     // point to the same block as the hash above)
//     checkpoint_number: u64,
//     /// How many blocks the tx is mortal for
//     period: u64,
// }

// 1. Nonce - A Counter Number
// rustnonce: Option<u64>
// What it is: A number that counts your transactions
// How it works:

// Your first transaction from an account = nonce 0
// Your second transaction = nonce 1
// Your third transaction = nonce 2
// Must go 0, 1, 2, 3... in order

// Why you need it:

// Blockchain knows which transaction came first
// Stops people from copying your transaction and sending it again

// Options:

// None = Let the blockchain pick the number automatically
// Some(5) = You pick the number (must be correct next number)

// 2. Mortality - When Transaction Expires
// rustmortality: Option<Mortality>
// What it is: How long your transaction stays valid
// Options:

// None = Transaction never expires (stays valid forever)
// Some(...) = Transaction expires after X blocks

// Why you need it:

// Prevents old transactions from running later by accident
// Keeps the network clean

// 3. Tip - How Much You Pay
// rusttip: u128
// What it is: The fee you pay to send the transaction
// How it works:

// Higher tip = your transaction gets processed faster
// Lower tip = your transaction waits longer
// Zero tip = your transaction might never get processed

// 1. tip_of_asset_id - Which Token to Pay With
// rusttip_of_asset_id: Option<T::AssetId>
// What it is: Which cryptocurrency you use to pay the fee
// Options:

// None = Use the main blockchain token (like DOT on Polkadot, ETH on Ethereum)
// Some(asset_id) = Use a different token (like USDC, custom tokens)

// Example:
// rusttip_of_asset_id: None,           // Pay with DOT
// tip_of_asset_id: Some(1234),     // Pay with token #1234 (maybe USDC)
// 2. tip - Base Fee Amount
// rusttip: u128
// What it is: The basic fee amount you pay
// Example:
// rusttip: 1000000,  // Pay 1,000,000 units (smallest denomination)
// 3. tip_of - Extra Tip Amount
// rusttip_of: u128
// What it is: Additional money you pay to get faster processing
// Example:
// rusttip: 1000000,      // Base fee: 1,000,000 units
// tip_of: 500000,    // Extra tip: 500,000 units
// // Total paid: 1,500,000 units
// Why separate tip and tip_of:

// tip = required minimum fee
// tip_of = optional extra payment for priority

// Mortality Structure Details
// What's Inside Mortality
// ruststruct Mortality<Hash> {
//     checkpoint_hash: Hash,
//     checkpoint_number: u64,
//     period: u64,
// }
// 1. checkpoint_hash - Starting Block ID
// rustcheckpoint_hash: Hash
// What it is: The unique ID of the block where your transaction's countdown starts
// Example:
// rustcheckpoint_hash: 0x1a2b3c4d...  // Block #1000's unique ID
// 2. checkpoint_number - Starting Block Number
// rustcheckpoint_number: u64
// What it is: The block number where your transaction's countdown starts
// Example:
// rustcheckpoint_number: 1000,  // Block number 1000
// Why both hash and number:

// Block number is easy to understand (1000, 1001, 1002...)
// Block hash prevents tricks/attacks (each block has unique fingerprint)
// Both must point to the same block

// 3. period - How Many Blocks Until Expiry
// rustperiod: u64
// What it is: How many blocks your transaction stays valid

// Your function calls: client.tx().create_signed_offline(...)
// create_signed_offline does the work:
// Takes your call, signer, and custom params (with nonce)
// Creates a signed transaction internally
// Wraps it in a SubmittableExtrinsic
// Returns: A SubmittableExtrinsic ready to submit

// The "offline" part means it doesn't fetch current data from the blockchain - it uses exactly what you provide (like your custom nonce), rather than automatically getting the latest nonce from the chain.

// What SubmittableExtrinsic Does in Simple Terms
// The function takes your transaction details, signs them, and gives you back something you can actually send to the blockchain.
// SubmittableExtrinsic is like a "ready-to-send blockchain transaction" - think of it as a sealed envelope that contains your transaction and is ready to be mailed to the blockchain.

// What You Can Do With It:
// Once you have a SubmittableExtrinsic, you can:

// // 1. Send it to the blockchain
// signed_tx.submit().await

// // 2. Send it and watch for confirmation
// signed_tx.submit_and_watch().await

// // 3. Get the transaction hash before sending
// let hash = signed_tx.hash()

// // 4. Check if it's valid
// signed_tx.validate()

//inside create_signed_offline: there is subxt_core::tx::create_signed
// in our main.rs there is client.tx().create_signed().

// hese are two completely different functions with the same name but different behavior:

// 1. subxt_core::tx::create_signed() (in tx_client.rs)
// Synchronous (no network calls)
// Works "offline" with data you already have
// Just does math/cryptography to create the transaction
// Returns immediately
// No .await needed

// 2. client.tx().create_signed() (in main.rs)
// Async (makes network calls)
// Connects to the blockchain to get current data (like nonce, block info)
// Takes time because it waits for blockchain responses
// Returns a Future that needs .await
// Requires .await

// we have two threads here:
// First for POST requests ✅
// Second is like a cron job syncing nonce from blockchain ✅

// Both threads access the same nonce_cache

// To prevent crashes or data races, we need:

// ✅ Arc → so both threads can share the cache
// ✅ Mutex → so they don’t write at the same time

// 🔁 You have two threads:
// 🧵 1. Main thread (handles HTTP POST requests)
// This is where users interact with your app.

// Each POST /do-something request:

// Reads the nonce from the shared cache

// Updates it if needed

// Proceeds with signing or sending a transaction

// ➡️ Runs whenever a user sends a request
// ➡️ May happen many times and even concurrently

// 🧵 2. Background sync thread (like a cron job)
// Every 30 seconds:

// Reads the latest nonces from the blockchain

// Updates the shared cache (nonce_cache)

// Implemented using:

// rust
// Copy
// Edit
// tokio::spawn(async move {
//     loop {
//         interval.tick().await;
//         sync_with_chain().await;
//     }
// });
// ➡️ Runs independently in the background
// ➡️ Yes — just like a cron job 🔄

// Do both threads really write to the data? Why do we need Mutex?

// Yes, BOTH threads are writing data to the same HashMap, which is why you need Mutex.

// ## Thread 1 (POST requests) - **WRITES** to cache:

// In `get_next_nonce()`:
// ```rust
// pub async fn get_next_nonce(&self, account_id: &AccountId32) -> Result<u64, ...> {
//     let mut cache = self.nonce_cache.lock().await;

//     // ... logic to determine nonce_to_use ...

//     // 🔥 WRITING TO CACHE HERE:
//     cache.insert(account_key, nonce_to_use + 1);  // ← WRITE OPERATION

//     Ok(nonce_to_use)
// }
// ```

// And in `reset_nonce()`:
// ```rust
// pub async fn reset_nonce(&self, account_id: &AccountId32, failed_nonce: u64) {
//     let mut cache = self.nonce_cache.lock().await;

//     // 🔥 WRITING TO CACHE HERE:
//     cache.insert(account_id.0, failed_nonce);  // ← WRITE OPERATION
// }
// ```

// ## Thread 2 (Background sync) - **WRITES** to cache:

// In `sync_with_chain()`:
// ```rust
// pub async fn sync_with_chain(&self) -> Result<(), ...> {
//     let mut cache = self.nonce_cache.lock().await;

//     for (account_key, cached_nonce) in cache.iter_mut() {
//         // ... check if chain is ahead ...

//         if chain_nonce > *cached_nonce {
//             // 🔥 WRITING TO CACHE HERE:
//             *cached_nonce = chain_nonce;  // ← WRITE OPERATION
//         }
//     }
// }
// ```

// ## What happens WITHOUT Mutex:

// ```rust
// // Thread 1: Trying to insert Alice's nonce = 5
// cache.insert([alice_key], 5);

// // Thread 2: At the SAME TIME trying to update Alice's nonce = 3
// *cached_nonce = 3;

// // RESULT: Data corruption!
// // Alice's nonce could end up as garbage data or cause a crash
// ```

// ## The race condition scenario:

// **Time 0:00**: Alice's nonce in cache = 2
// **Time 0:01**: Thread 1 reads Alice's nonce (2), calculates next = 3
// **Time 0:01**: Thread 2 reads from blockchain, Alice's nonce = 4
// **Time 0:02**: Thread 1 writes Alice's nonce = 3 (outdated!)
// **Time 0:02**: Thread 2 writes Alice's nonce = 4 (correct!)
// **Time 0:03**: Thread 1's write overwrites Thread 2's write
// **RESULT**: Alice's nonce = 3 (wrong! should be 4)

// ## Mutex prevents this:

// ```rust
// // Only ONE thread can hold the lock at a time
// let mut cache = self.nonce_cache.lock().await;  // 🔒 LOCKED

// // Thread 1 gets lock first:
// cache.insert([alice_key], 5);  // ✅ Safe write

// // 🔒 Lock released, now Thread 2 can get it:
// *cached_nonce = 3;  // ✅ Safe write (happens after Thread 1)
// ```

// **So yes, you need Mutex because both threads are modifying the same HashMap!**
