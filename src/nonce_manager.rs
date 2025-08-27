use std::{collections::HashMap, sync::Arc};
use subxt::{utils::AccountId32, OnlineClient, SubstrateConfig};
use tokio::sync::Mutex;

/// Production-grade nonce manager for blockchain transaction management
///
/// This manager provides thread-safe nonce caching and synchronization
/// with the blockchain to prevent nonce conflicts in concurrent transactions.
#[derive(Clone)]
pub struct NonceManager {
    /// Thread-safe cache mapping account keys to their next expected nonce
    nonce_cache: Arc<Mutex<HashMap<[u8; 32], u64>>>,
    /// Blockchain client for querying current nonces
    client: OnlineClient<SubstrateConfig>,
}

impl NonceManager {
    /// Creates a new nonce manager with an empty cache
    pub fn new(client: OnlineClient<SubstrateConfig>) -> Self {
        Self {
            nonce_cache: Arc::new(Mutex::new(HashMap::new())),
            client,
        }
    }

    /// Gets the next nonce for an account, handling synchronization with the blockchain.
    ///
    /// This method:
    /// 1. Checks the local cache for the account's next expected nonce
    /// 2. Queries the blockchain for the current nonce
    /// 3. Uses the higher of the two to avoid conflicts
    /// 4. Reserves the next nonce (nonce_to_use + 1) for future transactions
    ///
    /// # Arguments
    /// * `account_id` - The account ID to get the next nonce for
    ///
    /// # Returns
    /// * `Ok(nonce)` - The nonce to use for the next transaction
    /// * `Err(error)` - If there was an error querying the blockchain
    pub async fn get_next_nonce(
        &self,
        account_id: &AccountId32,
    ) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        let mut cache = self.nonce_cache.lock().await;

        // Get the account's current nonce from our cache
        // The .0 accesses the first (and only) field of the tuple struct
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

    /// Resets the nonce cache for an account to a specific value
    ///
    /// This is useful when a transaction fails and you want to reuse the same nonce
    /// for the next transaction attempt.
    ///
    /// # Arguments
    /// * `account_id` - The account ID to reset the nonce for
    /// * `failed_nonce` - The nonce that failed and should be reused
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

    /// Synchronizes the nonce cache with the blockchain state
    ///
    /// This method should be called periodically (e.g., every 30 seconds) to ensure
    /// the local cache stays in sync with the actual blockchain state. It:
    /// 1. Iterates through all cached accounts.
    /// 2. Queries the blockchain for each account's current nonce  
    /// 3. Updates the cache if the blockchain is ahead
    ///
    /// This prevents issues where external transactions (not from this service)
    /// advance the blockchain nonce ahead of our cache.
    ///
    /// # Returns
    /// * `Ok(())` - If synchronization completed successfully
    /// * `Err(error)` - If there was an error querying the blockchain
    pub async fn sync_with_chain(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut cache = self.nonce_cache.lock().await;

        // For each account in our cache, check if we're out of sync
        // HashMap {
        //     [1,2,3...32]: 10,    // Alice's account -> nonce 10
        //     [4,5,6...32]: 25,    // Bob's account   -> nonce 25
        //     [7,8,9...32]: 5,     // Charlie's account -> nonce 5
        // }
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
