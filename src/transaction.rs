// src/transaction.rs
use log;
use subxt::{ext::sp_core::sr25519::Pair, tx::PairSigner, OnlineClient, SubstrateConfig};

/// Creates a signed transaction with explicit nonce handling for blockchain submission
///
/// This function provides a robust approach to transaction signing by attempting
/// multiple methods to ensure successful transaction creation:
/// 1. Try using explicit nonce (preferred for proper nonce management)
/// 2. Fallback to default signing (subxt handles nonce internally)
///
/// The explicit nonce method is preferred because it allows our nonce manager
/// to maintain proper sequencing of transactions, preventing nonce conflicts
/// in concurrent transaction scenarios.
///
/// # Arguments
/// * `client` - The blockchain client for transaction creation
/// * `call` - The extrinsic call to be executed on the blockchain
/// * `signer` - The cryptographic signer (account) for the transaction
/// * `nonce` - The specific nonce value to use for this transaction
///
/// # Returns
/// * `Ok(SubmittableExtrinsic)` - A signed transaction ready for submission
/// * `Err(subxt::Error)` - If both signing methods fail
///
/// # Example Usage
/// ```rust
/// let call = chain_a::tx().template().do_something(42);
/// let pair_signer = PairSigner::new(pair);
/// let signed_tx = create_signed_transaction_with_nonce(&client, &call, &pair_signer, nonce).await?;
/// ```
pub async fn create_signed_transaction_with_nonce<Call>(
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
    log::info!(
        "🔧 Attempting to create signed transaction with nonce: {}",
        nonce
    );

    // Method 1: Try explicit nonce approach (preferred for nonce management)
    // This allows our nonce manager to maintain proper transaction sequencing
    if let Ok(tx) = try_with_explicit_nonce(client, call, signer, nonce).await {
        log::info!("✅ Successfully used explicit nonce method");
        return Ok(tx);
    }

    // Method 2: Fallback to default signing (subxt handles nonce internally)
    // This is less ideal because it bypasses our nonce management system
    // but provides a safety net if the explicit method fails
    log::warn!("⚠️ Explicit nonce failed, falling back to default signing method");

    match client
        .tx()
        .create_signed(call, signer, Default::default())
        .await
    {
        Ok(tx) => {
            log::info!("✅ Successfully used fallback signing method");
            Ok(tx)
        }
        Err(e) => {
            log::error!("❌ All signing methods failed: {:?}", e);
            Err(e)
        }
    }
}

/// Attempts to create a signed transaction with explicit nonce control
///
/// This is the preferred method for transaction signing as it allows precise
/// control over nonce values, which is essential for:
/// - Preventing nonce conflicts in concurrent transaction scenarios
/// - Maintaining proper transaction ordering
/// - Enabling transaction retry mechanisms with the same nonce
///
/// The function uses DefaultExtrinsicParamsBuilder to construct transaction
/// parameters with the specified nonce value, then creates an offline-signed
/// transaction that can be submitted to the blockchain.
///
/// # Arguments
/// * `client` - The blockchain client for accessing transaction APIs
/// * `call` - The extrinsic call to be signed
/// * `signer` - The cryptographic signer for the transaction
/// * `nonce` - The specific nonce value to embed in the transaction
///
/// # Returns
/// * `Ok(SubmittableExtrinsic)` - Successfully created signed transaction
/// * `Err(subxt::Error)` - If transaction creation fails (e.g., invalid parameters)
///
/// # Technical Details
/// This function creates an "offline" signed transaction, meaning it doesn't
/// query the blockchain for current state during signing. All required
/// parameters (including nonce) are provided explicitly.
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
    // Import the DefaultExtrinsicParamsBuilder for constructing transaction parameters
    use subxt::config::DefaultExtrinsicParamsBuilder;

    // Build transaction parameters with the explicit nonce
    // The builder pattern allows us to set specific parameters while
    // using defaults for others (like tip, mortality, etc.)
    let params = DefaultExtrinsicParamsBuilder::<SubstrateConfig>::new()
        .nonce(nonce) // Set the specific nonce value
        .build(); // Finalize the parameters

    // Create the signed transaction offline (without querying chain state)
    // This ensures the nonce we specify is actually used in the transaction
    client.tx().create_signed_offline(call, signer, params)
}

// .create_signed(call, signer, Default::default())

// Step 1: Nonce Management
// If you pass Default::default(), it queries the blockchain for current nonce
// If you pass explicit parameters, it uses your provided nonce
// Ensures transaction ordering and prevents replay attacks
// Step 2: Transaction Fee Calculation
// Calculates the transaction fee based on:
// Call complexity (weight)
// Current network conditions
// Any tip you want to include
// Step 3: Transaction Mortality/Lifetime
// Sets when the transaction expires (mortal vs immortal)
// Prevents old transactions from being replayed later
// Uses block hash for mortality period reference
// Step 4: Digital Signature Creation
// Uses the signer's private key to sign the transaction
// Creates cryptographic proof that you authorized this transaction
// Ensures transaction integrity and authenticity
// Step 5: Extrinsic Encoding
// Encodes everything into the binary format the blockchain expects
// Packages: call data + signature + nonce + fees + mortality

// Returns: A complete, ready-to-send transaction
