# Remote Aleo Prover

A lightweight Warp-based HTTP service that accepts Aleo `Authorization` payloads, executes them with SnarkVM, and returns proof metadata that can be used to assemble and broadcast transactions.

## Features

- ⚡ Async Warp server with graceful task offloading for heavy proving work.
- 🔐 Works with Mainnet V0 parameters out of the box.
- 🧪 Integration-tested endpoint covering a full contract execution flow.
- ⚙️ Optional CUDA acceleration: build with `cargo build --features cuda` on GPU machines.

## Prerequisites

- Rust 1.76+
- Aleo binaries / SDK (optional, for broadcasting)
- CUDA Toolkit + compatible GPU (optional, for `--features cuda`)

## Building & Running

```bash
# CPU build
cargo build --release

# Enable GPU proving (requires x86_64 + NVIDIA CUDA stack)
cargo build --release --features cuda

# Run the server (defaults to 0.0.0.0:3030)
./target/release/remote-prover
```

### Environment Variables & Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `PROVER_LISTEN_ADDR` | `0.0.0.0:3030` | Bind address for the HTTP server. |
| `MAX_CONCURRENT_PROOFS` | `available_parallelism()` | Maximum number of proofs executed concurrently (bounded with a semaphore). |
| `BROADCAST_ENDPOINT` | _unset_ | Default endpoint to POST proof results. Automatically triggers broadcasting unless the request sets `"broadcast": false`. |

These variables are optional today; the binary will fall back to the defaults shown above. You can define them in a `.env` file (using `dotenvy`) or export them in your shell.

```bash
export PROVER_LISTEN_ADDR=127.0.0.1:8080
export BROADCAST_ENDPOINT=https://api.aleo.org/v1/transactions/broadcast
```

## HTTP API

### `POST /prove`

Execute the provided authorization, returning proof metadata.

#### Request Body

```json
{
  "authorization": "AUTH_STRING",
  "broadcast": true,
  "broadcast_endpoint": "https://api.aleo.org/v1/transactions/broadcast"
}
```

- `authorization` – string-form serialization of an Aleo `Authorization`, typically produced by SnarkVM clients via `authorization.to_string()`.
- `broadcast` – optional boolean. If omitted, the server broadcasts when it has a default endpoint configured. Set to `false` to explicitly skip broadcasting.
- `broadcast_endpoint` – optional string. Overrides the default endpoint for this request and implicitly enables broadcasting.

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
    "endpoint": "https://api.aleo.org/v1/transactions/broadcast",
    "status": 200,
    "success": true,
    "response": "{\"tx_id\":\"...\"}"
  }
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

When `BROADCAST_ENDPOINT` is configured, the service will POST a JSON payload containing the proof summary plus the original authorization to the specified endpoint unless the request opts out. Non-2xx responses are logged but do not break the HTTP request.

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
export BROADCAST_ENDPOINT=https://api.aleo.org/v1/transactions/broadcast
./target/release/remote-prover &

AUTH=$(./authorize_call.sh)
curl -X POST \
  -H "Content-Type: application/json" \
  -d "{\"authorization\": \"${AUTH}\", \"broadcast\": true}" \
  http://localhost:3030/prove
# The prover will forward the proof + authorization to the broadcast endpoint on success.
```

### 3. Override the Broadcast Endpoint Per Request

```bash
AUTH=$(./authorize_call.sh)
curl -X POST \
  -H "Content-Type: application/json" \
  -d "{\"authorization\": \"${AUTH}\", \"broadcast_endpoint\": \"https://testnet.aleo.org/v1/tx\"}" \
  http://localhost:3030/prove
# Uses the provided endpoint even if the server has a different default.
```

### 4. GPU-Accelerated Run (CUDA)

```bash
export LD_LIBRARY_PATH=/usr/local/cuda/lib64:$LD_LIBRARY_PATH
cargo run --release --features cuda
# Submit jobs as usual via curl
```

> **Heads up:** CUDA support in SnarkVM is currently limited to x86_64 targets with NVIDIA GPUs and the CUDA toolkit installed. Builds will now fail early if you enable `--features cuda` on unsupported architectures (for example Apple Silicon Macs).

## Broadcasting Transactions Manually

If you prefer to broadcast yourself, combine the response data with SnarkVM to build a full `Transaction` object (using the trace and response returned by the prover), then submit via Aleo’s REST interface:

```bash
curl -X POST \
  -H "Content-Type: application/json" \
  -d '{
    "transaction": "{\"id\":\"...\",\"proof\":...}"
  }' \
  https://api.aleo.org/v1/transactions/broadcast
```

## Testing

```bash
cargo test                    # Includes integration test for /prove
cargo test --features cuda    # GPU path smoke test
```

The integration test `tests/prover_api.rs` demonstrates programmatic authorization generation and HTTP submission end-to-end.

## Roadmap

- 🔄 Support batching multiple authorizations in one request
- 🔒 JWT-based authentication for prover access
- 🌐 Multiple Aleo network configurations
- 📤 Richer broadcast payload (full transaction assembly)

## License

Apache 2.0
