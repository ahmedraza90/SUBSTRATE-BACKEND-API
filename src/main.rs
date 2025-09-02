// src/main.rs
use axum::{
    routing::{get, post},
    Router,
};
use log;
use subxt::{OnlineClient, SubstrateConfig};
use tower_http::cors::CorsLayer;

// Import our modules
mod handlers;
mod nonce_manager;
mod transaction;
use handlers::{
    do_something_handler, get_latest_events, get_storage_handler, health_check, AppState,
};
use nonce_manager::NonceManager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Force logging level
    std::env::set_var("RUST_LOG", "info");

    // This line initializes the logging system for your application. Let me explain what it does:
    // After this line, all log::info!(), log::error!(), etc. will work

    env_logger::init();

    // Connect to Chain A
    let client = OnlineClient::<SubstrateConfig>::from_url("ws://localhost:9944").await?;
    log::info!("Connected to Chain A at ws://localhost:9944");

    // Create nonce manager
    let nonce_manager = NonceManager::new(client.clone());

    // Start background sync task
    let sync_manager = nonce_manager.clone();

    // This runs on the SAME thread as your main code
    // But switches back and forth very quickly

    // 🧵 ONE Physical Thread:
    // ┌─────────────────────────────────────────────────────────────┐
    // │ Time: 0-10ms   │ Time: 10-20ms │ Time: 20-30ms │ Time: 30-40ms │
    // │ Main HTTP      │ Background    │ Main HTTP     │ Background    │
    // │ Request        │ Sync Task     │ Request       │ Sync Task     │
    // └─────────────────────────────────────────────────────────────┘
    tokio::spawn(async move {
        // ✅ This is like BUYING a kitchen timer and SETTING it to 30 seconds
        // ⏰ The timer is now SET UP but hasn't started counting yet
        // 🚀 This happens INSTANTLY - no waiting involved
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            // ⏳ This is like PRESSING START on the kitchen timer and WAITING for it to ring
            // 😴 Your code STOPS HERE and waits...
            // ⏰ After 30 seconds, the timer "rings" and your code continues
            interval.tick().await; // ← YIELDS control back to main thread
                                   //  "I'm waiting, you can do other work"

            if let Err(e) = sync_manager.sync_with_chain().await {
                // ← YIELDS during network I/O
                // "I'm waiting for network, you handle HTTP"
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
        .with_state(state); // ← This attaches shared state to the router

    // Start the server
    // What it is: A tool that listens for incoming network connections
    // .bind("127.0.0.1:3001") - Where to Listen
    // .await: Wait for the listener to be ready
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001").await?;
    log::info!("Backend API server running on http://127.0.0.1:3001");

    // This line starts your web server and is the final step that makes your API accessible to the world. Let me break it down:
    axum::serve(listener, app).await?;

    Ok(())
}

// 1. What is AccountId32?
// pub struct AccountId32(pub [u8; 32]);
// It’s a tuple struct: a wrapper around a 32-byte array.
// Inside, the real account identifier is [u8; 32].

// nonce_cache: Arc<Mutex<HashMap<[u8; 32], u64>>>,

// let account_key = account_id.0;
// let cached_nonce = cache.get(&account_key).copied();  //.copied() converts Option<&u64> to
// Option<u64> by copying the dereferenced value

// 2. Why not use AccountId32 directly as the HashMap key?
// In theory, you can use AccountId32 as a key.
// But for that, AccountId32 must implement the Eq and Hash traits (because HashMap needs hashing and equality checks for keys).

// If the codebase (or imported library) doesn’t derive Hash/Eq for AccountId32, you can’t directly use it in a HashMap.
// [u8; 32] already implements Hash, Eq, PartialEq, Clone, Copy, so it’s a convenient key.

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
// One for http requests ✅
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

// .lock().await does this:
// 1. Waits for the Mutex to be available
// 2. Locks the Mutex (blocks other threads)
// 3. Returns a MutexGuard<HashMap<...>>

// The MutexGuard automatically dereferences to HashMap!
// let mut cache = self.nonce_cache.lock().await;
// cache is now: MutexGuard<HashMap<[u8; 32], u64>>
// But it behaves like: &mut HashMap<[u8; 32], u64>

// for (account_key, cached_nonce) in cache.iter_mut() {
//  ^^^^^^^^^^^^  ^^^^^^^^^^^^^     ^^^^^^^^^^^^^^^
//  |             |                 |
//  |             |                 This calls HashMap::iter_mut()
//  |             |                 Returns iterator over (&K, &mut V)
//  |             |
//  |             This is: &mut u64 (mutable reference to value)
//  |
//  This is: &[u8; 32] (reference to key)
// HashMap keys CANNOT be modified because:
// 1. Keys determine WHERE values are stored (hash bucket)
// 2. Changing a key would break the HashMap's internal structure
// 3. You'd need to remove + re-insert to "change" a key

// 🎯 Why Not iter() Instead of iter_mut()?
// If we used iter() instead of iter_mut():
// for (account_key, cached_nonce) in cache.iter() {
// //                 ^^^^^^^^^^^^^
// //                 This would be &u64 (immutable!)

//     if chain_nonce > *cached_nonce {  // ✅ Reading works
//         *cached_nonce = chain_nonce;  // ❌ COMPILE ERROR!
//         //              ^^^^^^^^^^^^
//         //              Cannot modify through immutable reference!
//     }
// }

// Json is an Axum wrapper that:
// Takes your Rust struct
// Converts it to JSON automatically

// ❌ Less efficient - creates String even when not needed
// let signer_seed = payload.signer.unwrap_or("//Alice".to_string());

// ✅ More efficient - only creates String when None
// let signer_seed = payload.signer.unwrap_or_else(|| "//Alice".to_string());
