# Remote Aleo Prover

A lightweight Warp-based HTTP service that accepts Aleo `Authorization` payloads, executes them with SnarkVM, and returns proof metadata that can be used to assemble and broadcast transactions.

## Features

- ⚡ Async Warp server with graceful task offloading for heavy proving work.
- 🔐 Works with Mainnet V0 parameters out of the box.
- 🧪 Integration-tested endpoint covering a full contract execution flow.

## Prerequisites

- Rust 1.76+
- Aleo binaries / SDK (optional, for broadcasting)

## Building & Running

```bash
cargo build --release

# Run the server (defaults to 0.0.0.0:3030)
./target/release/remote-prover
```

### Generating an Authorization Locally

If you do not want to rely on the Leo CLI, the repository now ships with a thin
wrapper around SnarkVM that emits an authorization string suitable for the
`/prove` endpoint. Provide the function name, private key, and inputs using Leo
literal syntax. The first argument can either be a local `.aleo` file or an
on-chain program ID:

```bash
./scripts/authorize_call.sh build/main.aleo set_data_sgx APrivateKey1... \
  "1u64" "aleo1..."
# Fetch the latest deployed program from Provable Testnet
AUTHORIZE_NETWORK=testnet ./scripts/authorize_call.sh veru_oracle_v2.aleo set_data_sgx \
  APrivateKey1...
```

Under the hood this script executes `cargo run --bin authorize`, so the first
invocation will compile the binary. You can pass `AUTHORIZE_RELEASE=1` to the
script to use the release profile, and `PRINT_ACCOUNT=1` to print the derived
account address to stderr for verification. When targeting on-chain programs,
set `AUTHORIZE_NETWORK` to `mainnet`, `testnet`, or `canary` (defaults to
`testnet`), optionally override the API base with `AUTHORIZE_API_BASE`, and
specify a particular program edition by exporting `AUTHORIZE_EDITION`.

### Environment Variables & Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `PROVER_LISTEN_ADDR` | `0.0.0.0:3030` | Bind address for the HTTP server. |
| `MAX_CONCURRENT_PROOFS` | `available_parallelism()` | Maximum number of proofs executed concurrently (bounded with a semaphore). |
| `NETWORK` | `testnet` | Selects the Provable Explorer network used for automatic program fetching and default broadcasting (`mainnet`, `testnet`, `canary`). |
| `ENFORCE_PROGRAM_EDITIONS` | `true` | Validates that imported programs are on a non-stale edition once the network has upgraded (mirrors snarkVM's `check_valid_edition`). Disable only for unit tests or local experimentation. |
| `REST_ENDPOINT_OVERRIDE` | *(unset)* | Overrides the REST endpoint used for state queries (expects either a full URI or a static JSON payload shaped like `{"state_root":"…","height":0}`). Useful for tests and air-gapped deployments. |

These variables are optional today; the binary will fall back to the defaults shown above. You can define them in a `.env` file (using `dotenvy`) or export them in your shell.

```bash
export PROVER_LISTEN_ADDR=127.0.0.1:8080
export NETWORK=testnet
```

## HTTP API

### `POST /prove`

Execute the provided authorization, returning proof metadata.

#### Request Body

```json
{
  "authorization": "AUTH_STRING",
  "fee_authorization": "FEE_AUTH_STRING",
  "broadcast": true,
  "network": "mainnet"
}
```

- `authorization` – string-form serialization of an Aleo `Authorization`, typically produced by SnarkVM clients via `authorization.to_string()`.
- `fee_authorization` – optional string or JSON-encoded authorization that produces the fee transition for the transaction. Provide this when the main execution does not already include a fee transition (common for application calls that rely on a wallet's credits).
- `broadcast` – optional boolean. If omitted, the server broadcasts when it has a default endpoint configured. Set to `false` to explicitly skip broadcasting.
- `network` – optional string (`"mainnet"`, `"testnet"`, or `"canary"`). Overrides the server's configured network for fetching programs and selecting the broadcast endpoint.

#### Response Examples

##### Success (200)

```json
{
  "status": "success",
  "summary": {
    "output_ids": [
      "Public(Field(…))"
    ],
    "outputs": [
      "Value::Plaintext(u32.public(12))"
    ],
    "transitions": 1,
    "is_fee": false
  },
  "broadcast": {
    "requested": true,
    "endpoint": "https://api.explorer.provable.com/v2/testnet/transaction/broadcast",
    "status": 200,
    "success": true,
    "response": "{\"tx_id\":\"...\"}"
  },
  "fee": {
    "kind": "private",
    "transition_id": "...",
    "amount_microcredits": "1200000",
    "base_microcredits": "1000000",
    "priority_microcredits": "200000",
    "payer": "aleo1...",
    "global_state_root": "...",
    "num_finalize_operations": 0
  },
  "transaction_payload": "{\"type\":\"execute\",...}"
}
```

##### Error (400)

```json
{
  "status": "error",
  "message": "Error parsing authorization: Invalid signature"
}
```

##### Worker Failure (500)

```json
{
  "status": "error",
  "message": "Worker panicked while proving: shutdown"
}
```

By default the service POSTs a JSON payload containing the serialized transaction (`transaction_payload`) to the canonical Provable Explorer endpoint for the configured `NETWORK` unless the request opts out (`"broadcast": false`). Non-2xx responses are logged but do not break the HTTP request.

## cURL Examples

### 1. CPU Proof Submission

```bash
AUTH=$(./authorize_call.sh) # Any script that prints an Authorization string
curl -X POST \
  -H "Content-Type: application/json" \
  -d "{\"authorization\": \"${AUTH}\"}" \
  http://localhost:3030/prove
```

### 2. Proof Submission with Broadcast

```bash
export NETWORK=mainnet
./target/release/remote-prover &

AUTH=$(./authorize_call.sh)
curl -X POST \
  -H "Content-Type: application/json" \
  -d "{\"authorization\": \"${AUTH}\", \"broadcast\": true}" \
  http://localhost:3030/prove
# The prover will forward the proof + authorization to the canonical mainnet endpoint on success.
```

### 3. Override the Network Per Request

```bash
AUTH=$(./authorize_call.sh)
curl -X POST \
  -H "Content-Type: application/json" \
  -d "{\"authorization\": \"${AUTH}\", \"network\": \"testnet\"}" \
  http://localhost:3030/prove
# Fetches programs and broadcasts using the testnet explorer regardless of the server default.
```

### 4. Skip Broadcasting for a Single Request

```bash
AUTH=$(./authorize_call.sh)
curl -X POST \
  -H "Content-Type: application/json" \
  -d "{\"authorization\": \"${AUTH}\", \"broadcast\": false}" \
  http://localhost:3030/prove
# Explicitly opts out of broadcasting while still producing the proof summary.
```

## Broadcasting Transactions Manually

If you prefer to broadcast yourself, you can take the `transaction_payload` string returned by `/prove` and submit it directly to Aleo’s REST interface:

```bash
curl -X POST \
  -H "Content-Type: application/json" \
  -d '{
    "transaction": "{\"type\":\"execute\",\"id\":\"...\",...}"
  }' \
  https://api.explorer.provable.com/v2/testnet/transaction/broadcast
```

When you omit `fee_authorization`, the prover expects that the supplied `authorization` already contains a fee transition. If the execution requires a separate fee payer, generate a second authorization that spends the payer's credits and attach it via `fee_authorization`; the prover will execute both, merge them into a single transaction, and include fee metadata in the response for easier bookkeeping.

## Testing

```bash
cargo test                    # Includes integration test for /prove
```

The integration test `tests/prover_api.rs` demonstrates programmatic authorization generation and HTTP submission end-to-end.

## Roadmap

- 🔄 Support batching multiple authorizations in one request
- 🔒 JWT-based authentication for prover access
- 🌐 Multiple Aleo network configurations
- 📤 Richer broadcast payload (full transaction assembly)

## License

Apache 2.0
