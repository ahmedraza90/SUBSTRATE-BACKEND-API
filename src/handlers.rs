// src/handlers.rs
//
// HTTP request handlers for the Substrate blockchain API
//
// This module contains all the REST API endpoints that external clients
// can use to interact with the blockchain. Each handler manages:
// - Request validation and parsing
// - Blockchain interaction via subxt
// - Response formatting and error handling
// - Nonce management for transactions

use axum::{extract::State, http::StatusCode, response::Json};
use log;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use subxt::{
    ext::sp_core::{sr25519::Pair, Pair as PairTrait},
    tx::PairSigner,
    utils::AccountId32,
    OnlineClient, SubstrateConfig,
};

use crate::nonce_manager::NonceManager;
use crate::transaction::create_signed_transaction_with_nonce;

// Include the generated runtime types from the blockchain's metadata
// This macro generates Rust types and APIs based on the actual runtime
#[subxt::subxt(runtime_metadata_path = "src/metadata.scale")]
pub mod chain_a {}

/// Request payload for the do_something endpoint
///
/// This represents the JSON structure that clients send when calling
/// the /do-something endpoint to submit transactions to the blockchain.
#[derive(Debug, Serialize, Deserialize)]
pub struct DoSomethingRequest {
    /// The value to store on the blockchain (required)
    pub value: u32,
    /// Optional signer account seed (defaults to "//Alice" if not provided)
    /// Example seeds: "//Alice", "//Bob", "//Charlie", or custom private keys
    pub signer: Option<String>,
}

/// Response payload for the do_something endpoint
///
/// This structure provides comprehensive information about the transaction
/// result, including success status, blockchain hashes, and error details.
#[derive(Debug, Serialize, Deserialize)]
pub struct DoSomethingResponse {
    /// Whether the transaction was successfully submitted and finalized
    pub success: bool,
    /// The transaction hash if successfully submitted (hex string)
    pub transaction_hash: Option<String>,
    /// The block hash where the transaction was included (hex string)
    pub block_hash: Option<String>,
    /// Error message if the transaction failed at any stage
    pub error: Option<String>,
}

/// Response payload for the get_storage endpoint
///
/// This structure contains the current value stored on the blockchain
/// along with the block hash where the value was queried from.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetStorageResponse {
    /// The current value stored on the blockchain (None if not set)
    pub value: Option<u32>,
    /// The block hash where this value was queried from
    pub block_hash: String,
}

/// Application state shared across all HTTP handlers
///
/// This struct contains the core dependencies that handlers need to
/// interact with the blockchain and manage transaction state:
/// - Blockchain client for queries and transaction submission
/// - Nonce manager for preventing transaction conflicts
///
/// The Clone trait allows this state to be efficiently shared across
/// multiple concurrent HTTP requests without expensive copying.
#[derive(Clone)]
pub struct AppState {
    /// The subxt client for blockchain communication
    pub client: OnlineClient<SubstrateConfig>,
    /// Production-grade nonce manager for transaction sequencing
    pub nonce_manager: NonceManager,
}

/// Health check endpoint for service monitoring
///
/// This endpoint provides a simple way for load balancers, monitoring
/// systems, or clients to verify that the API service is running and
/// responsive. It returns basic service information without requiring
/// any blockchain interaction.
///
/// # Returns
/// JSON object with service status and identification information
///
/// # Example Response
/// ```json
/// {
///   "status": "healthy",
///   "service": "Chain A Backend API"
/// }
/// ```

pub async fn health_check() -> Json<HashMap<String, String>> {
    let mut response = HashMap::new();
    response.insert("status".to_string(), "healthy".to_string());
    response.insert("service".to_string(), "Chain A Backend API".to_string());
    Json(response)
}

/// Handles the /do-something endpoint for submitting transactions to the blockchain
///
/// This is the main transaction endpoint that:
/// 1. Validates the request payload and signer
/// 2. Gets the next available nonce for the account
/// 3. Creates and signs a blockchain transaction
/// 4. Submits the transaction and waits for finalization
/// 5. Returns the transaction result with hashes and status
///
/// The endpoint uses production-grade nonce management to prevent conflicts
/// when multiple transactions are submitted concurrently for the same account.
///
/// # Request Format
/// POST /do-something
/// ```json
/// {
///   "value": 42,
///   "signer": "//Alice"  // optional, defaults to //Alice
/// }
/// ```
///
/// # Response Format
/// ```json
/// {
///   "success": true,
///   "transaction_hash": "0x...",
///   "block_hash": "0x...",
///   "error": null
/// }
/// ```
///
/// # Arguments
/// * `state` - Shared application state (client + nonce manager)
/// * `payload` - JSON request body with value and optional signer
///
/// # Returns
/// JSON response with transaction result or error details
pub async fn do_something_handler(
    State(state): State<AppState>,
    Json(payload): Json<DoSomethingRequest>,
) -> Result<Json<DoSomethingResponse>, StatusCode> {
    log::info!(
        "📥 Received do_something request with value: {} from signer: {:?}",
        payload.value,
        payload.signer
    );

    // Parse and validate the signer account
    // Default to Alice if no signer is provided in the request
    //This is a static method that converts a seed string into a cryptographic key pair for blockchain transactions.
    let signer_seed = payload.signer.unwrap_or_else(|| "//Alice".to_string());
    let signer = match Pair::from_string(&signer_seed, None) {
        Ok(pair) => pair,
        Err(_) => {
            log::error!("❌ Invalid signer seed provided: {}", signer_seed);
            return Ok(Json(DoSomethingResponse {
                success: false,
                transaction_hash: None,
                block_hash: None,
                error: Some("Invalid signer".to_string()),
            }));
        }
    };
    // Keypair {
    //     secret: SecretKey,  ✅ Has private key for signing
    //     public: PublicKey,  ✅ Has public key for verification
    // }

    // Convert the cryptographic public key to an AccountId32 for nonce management
    let account_id = AccountId32::from(signer.public());

    // Get the next nonce for this account using our production nonce manager
    // This prevents nonce conflicts when multiple transactions are submitted concurrently
    let nonce = match state.nonce_manager.get_next_nonce(&account_id).await {
        Ok(nonce) => nonce,
        Err(e) => {
            log::error!(
                "❌ Failed to get nonce for account {:?}: {:?}",
                account_id,
                e
            );
            return Ok(Json(DoSomethingResponse {
                success: false,
                transaction_hash: None,
                block_hash: None,
                error: Some(format!("Failed to get nonce: {:?}", e)),
            }));
        }
    };

    // Create the blockchain extrinsic call
    // This represents the actual function call that will be executed on-chain
    let call = chain_a::tx().template().do_something(payload.value);

    // Wrap the cryptographic pair in a PairSigner for transaction signing
    // it implement Signer<SubstrateConfig> trait that using this wrapper thats why using it instead of directly using struct if key pair.
    let pair_signer = PairSigner::new(signer);

    // Create the signed transaction with explicit nonce control
    // This ensures our nonce manager maintains proper transaction sequencing
    let signed_tx =
        match create_signed_transaction_with_nonce(&state.client, &call, &pair_signer, nonce).await
        {
            Ok(tx) => tx,
            Err(e) => {
                // Reset nonce since we failed to create transaction
                // This allows the same nonce to be reused for the next attempt
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

    // Submit the transaction to the blockchain and monitor its progress
    // What This Does:
    // Submits the signed transaction to the blockchain mempool
    // Starts monitoring the transaction's progress through the blockchain
    // Returns immediately after submission (doesn't wait for finalization)
    // Success case:
    // Ok(progress) -> TxProgress<SubstrateConfig, OnlineClient<SubstrateConfig>>
    // What TxProgress Contains:
    // TxProgress {
    //     transaction_hash: H256,           // Unique transaction identifier
    //     block_subscription: Stream,       // Real-time block updates
    //     client: OnlineClient,            // For additional queries
    //     // ... internal monitoring state
    // }
    match signed_tx.submit_and_watch().await {
        Ok(progress) => {
            // Wait for the transaction to be included in a finalized block
            // This ensures the transaction is permanently recorded on the blockchain
            // What wait_for_finalized_success() Does:
            // Monitors blockchain for the transaction to be included in a block
            // Waits for finalization (block becomes permanent, not just proposed)
            // Validates success (transaction didn't fail during execution)
            // Collects events emitted by the transaction
            // Success case:
            // Ok(events) -> TxInBlock<SubstrateConfig, OnlineClient<SubstrateConfig>>
            // TxInBlock {
            //     block_hash: H256,                // Hash of the block containing transaction
            //     extrinsic_hash: H256,           // Transaction hash
            //     extrinsic_index: u32,           // Position in block
            //     events: Events,                 // All events emitted by this transaction
            //     // ... other metadata
            // }

            match progress.wait_for_finalized_success().await {
                Ok(events) => {
                    // Extract the transaction hash from the finalized events
                    let tx_hash = format!("{:?}", events.extrinsic_hash());

                    // Get the hash of the block containing our transaction
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
                    // Transaction was submitted but failed during execution
                    // Reset nonce so it can be reused for retry attempts
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
            // Failed to submit transaction to the mempool
            // Reset nonce so it can be reused for retry attempts
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
}

/// Handles the /storage endpoint for querying blockchain state
///
/// This endpoint allows clients to read the current value stored on the
/// blockchain without submitting any transactions. It queries the latest
/// finalized block to ensure the returned data is permanently committed.
///
/// The endpoint demonstrates how to:
/// - Query blockchain storage at a specific block
/// - Handle storage values that might not exist (Option<T>)
/// - Return block metadata along with storage data
///
/// # Request Format
/// GET /storage
/// (No request body required)
///
/// # Response Format
/// ```json
/// {
///   "value": 42,  // or null if no value is stored
///   "block_hash": "0x..."
/// }
/// ```
///
/// # Arguments
/// * `state` - Shared application state containing the blockchain client
///
/// # Returns
/// JSON response with the storage value and block hash, or 500 on error
pub async fn get_storage_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<Json<GetStorageResponse>, StatusCode> {
    // Get the latest finalized block to ensure data consistency
    // Finalized blocks are guaranteed to be permanent and won't be reverted
    let latest_block = match state.client.blocks().at_latest().await {
        Ok(block) => block,
        Err(e) => {
            log::error!("❌ Failed to get latest block: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Create a storage query for the "something" value in the template pallet
    // This corresponds to the storage item defined in the blockchain runtime
    // Create a storage query for the "something" value in the template pallet
    // This corresponds to the storage item defined in the blockchain runtime
    // In your blockchain runtime (Substrate pallet):
    // #[frame_support::pallet]
    // pub mod pallet {
    //     #[pallet::storage]
    //     pub type Something<T> = StorageValue<_, u32, OptionQuery>;
    //     //       ^^^^^^^^^                              ^^^^^
    //     //       Becomes something()                    Returns Option<u32>

    //     #[pallet::call]
    //     impl<T: Config> Pallet<T> {
    //         pub fn do_something(value: u32) -> DispatchResult {
    //             Something::<T>::put(value);  // Stores the value
    //             Ok(())
    //         }
    //     }
    // }

    //     Blockchain Runtime
    // ├── Template Pallet
    // │   ├── Storage Items:
    // │   │   └── Something: Option<u32>  ← This becomes .something()
    // │   ├── Functions:
    // │   │   └── do_something(value)     ← This becomes .do_something(value)
    // │   └── Events:
    // │       └── SomethingStored         ← This becomes events::SomethingStored
    // └── Other Pallets...

    // Generated Rust API (chain_a):
    // ├── storage().template().something()     ← Read the Something storage
    // ├── tx().template().do_something(value)  ← Call the do_something function
    // └── template::events::SomethingStored    ← Listen for SomethingStored events
    //     let storage_query = chain_a::storage().template().something();

    // Execute the storage query at the specific block
    // The result is Option<u32> since the storage might not contain a value

    // queries available:
    // chain_a::storage().template().something()      // Read "something" value
    // chain_a::storage().system().account()          // Read account info
    // chain_a::storage().balances().total_issuance() // Read total supply

    // // Transaction calls available:
    // chain_a::tx().template().do_something(42)      // Call do_something function
    // chain_a::tx().balances().transfer(dest, amount) // Transfer tokens

    // // Events available:
    // chain_a::template::events::SomethingStored     // Template events
    // chain_a::balances::events::Transfer            // Balance events

    // two-step process for reading blockchain storage. Let me explain why we need both steps:

    // 1. Create a storage query for the specific item we want to read (1--// WHAT to fetch (reusable)) (2--// The query knows what type to expect)
    // Like writing an address on paper:
    // let address = "123 Main Street, Springfield, Ohio"
    // You have the address, but you haven't gone there yet!
    // 2. Execute the storage query at a specific block to get the value (// WHEN/WHERE to fetch from (can change))

    //     What If We Combined Them?
    // If we tried to do it in one step, it might look like:

    // Problems with this approach:

    // Less flexible (can't reuse the query)
    // Harder to understand what's happening
    // Less type-safe
    // Can't easily query the same data from different blocks

    let storage_query = chain_a::storage().template().something();

    // Execute the storage query at the specific block
    // The result is Option<u32> since the storage might not contain a value
    match latest_block.storage().fetch(&storage_query).await {
        Ok(value) => Ok(Json(GetStorageResponse {
            value,
            block_hash: format!("{:?}", latest_block.hash()),
        })),
        Err(e) => {
            log::error!("❌ Failed to fetch storage: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Handles the /events endpoint for retrieving recent blockchain events
///
/// This endpoint queries the latest finalized block for events emitted by
/// the template pallet. It's useful for monitoring blockchain activity and
/// tracking the results of submitted transactions.
///
/// The endpoint demonstrates how to:
/// - Access events from a specific block
/// - Filter events by pallet and event type
/// - Parse event data into readable formats
/// - Handle event parsing errors gracefully
///
/// # Request Format
/// GET /events
/// (No request body required)
///
/// # Response Format
/// ```json
/// [
///   "SomethingStored: value=42, who=5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
///   "SomethingStored: value=123, who=5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty"
/// ]
/// ```
///
/// # Arguments
/// * `state` - Shared application state containing the blockchain client
///
/// # Returns
/// JSON array of event descriptions, or 500 on error
pub async fn get_latest_events(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<Json<Vec<String>>, StatusCode> {
    // Get latest finalized block to ensure we're reading permanent data
    let latest_block = match state.client.blocks().at_latest().await {
        Ok(block) => block,
        Err(e) => {
            log::error!("❌ Failed to get latest block: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Retrieve all events that occurred in this block
    let events = match latest_block.events().await {
        Ok(events) => events,
        Err(e) => {
            log::error!("❌ Failed to get block events: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut event_list = Vec::new();

    // for event in events.iter() {
    // At this point:
    // event: Result<EventDetails, Error>
    // This could be Ok(actual_event_data) or Err(some_error)
    // }

    // if let Ok(event) = event {
    //        ^^^^^    ^^^^^
    //        |        |
    //        |        └── Original variable (Result type)
    //        └── NEW variable name (extracted EventDetails)
    // }
    // Iterate through all events in the block and filter for template pallet events
    for event in events.iter() {
        if let Ok(event) = event {
            // Attempt to parse this event as a SomethingStored event from the template pallet
            if let Ok(template_event) =
                event.as_event::<chain_a::template::events::SomethingStored>()
            {
                if let Some(stored_event) = template_event {
                    // Format the event data into a human-readable string
                    event_list.push(format!(
                        "SomethingStored: value={}, who={:?}",
                        stored_event.something, stored_event.who
                    ));
                }
            }
            // Additional event types can be parsed here as needed
            // Example: event.as_event::<chain_a::template::events::SomethingElse>()
        }
    }

    // In your blockchain runtime (e.g., pallets/template/src/lib.rs):
    // #[pallet::event]
    // #[pallet::generate_deposit(pub(super) fn deposit_event)]
    // pub enum Event<T: Config> {
    //     /// Event emitted when something is stored
    //     SomethingStored {
    //         something: u32,
    //         who: T::AccountId,
    //     },
    //     /// Event emitted when value is updated
    //     ValueUpdated {
    //         old_value: u32,
    //         new_value: u32,
    //         who: T::AccountId,
    //     },
    //     /// Another example event
    //     SomethingCleared {
    //         who: T::AccountId,
    //     },
    // }

    // After running the subxt macro, you can explore available events:

    Ok(Json(event_list))
}
