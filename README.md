# Substrate API Backend

A production-ready REST API backend for interacting with Substrate blockchains.

## Project Structure

```
src/
├── main.rs                 # Application entry point
├── lib.rs                  # Library root
├── config/
│   ├── mod.rs             # Configuration module exports
│   └── server.rs          # Server configuration and client setup
├── handlers/
│   ├── mod.rs             # Handler module exports
│   ├── health.rs          # Health check endpoints
│   └── blockchain.rs      # Blockchain-related endpoints
├── models/
│   ├── mod.rs             # Model module exports
│   ├── requests.rs        # Request data structures
│   ├── responses.rs       # Response data structures
│   ├── app_state.rs       # Application state
│   └── chain.rs           # Blockchain runtime definitions
├── services/
│   ├── mod.rs             # Service module exports
│   ├── nonce_manager.rs   # Nonce management service
│   └── transaction_service.rs # Transaction handling utilities
└── utils/
    ├── mod.rs             # Utility module exports
    └── routes.rs          # Route configuration
```

## Features

- **Modular Architecture**: Clean separation of concerns with dedicated modules
- **Nonce Management**: Production-grade nonce handling for concurrent transactions
- **Type Safety**: Leverages Rust's type system and Substrate's type-safe APIs
- **Error Handling**: Comprehensive error handling with detailed logging
- **CORS Support**: Ready for web frontend integration

## API Endpoints

- `GET /health` - Health check
- `POST /do-something` - Submit blockchain transaction
- `GET /get-storage` - Query blockchain storage
- `GET /latest-events` - Retrieve recent blockchain events

## Running the Application

```bash
cargo run
```

The server will start on `http://127.0.0.1:3001`

## Testing

Use the provided test script:

```bash
./simple_test.sh
```
