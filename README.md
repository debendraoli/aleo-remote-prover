# Remote Aleo Prover

A lightweight HTTP service that accepts Aleo `Authorization` payloads, generates proofs using SnarkVM, and broadcasts transactions to the Aleo network.

## Building & Running

```bash
cargo build --release
./target/release/remote-prover
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `PROVER_LISTEN_ADDR` | `0.0.0.0:3030` | HTTP server bind address |
| `MAX_CONCURRENT_PROOFS` | CPU cores | Maximum concurrent proof operations |
| `ENFORCE_PROGRAM_EDITIONS` | `true` | Validate program editions against network |
| `ENDPOINT` | `https://api.explorer.provable.com` | REST endpoint for state queries |

## API

### `GET /`

Health check endpoint.

### `POST /prove`

Generate proof and optionally broadcast transaction.

**Request:**

```json
{
  "authorization": {},
  "fee_authorization": {},
  "broadcast": true
}
```

- `authorization` – Aleo authorization object (required)
- `fee_authorization` – Fee authorization object (optional)
- `broadcast` – Broadcast transaction after proving (default: `true`)

**Response:**

```json
{
  "status": "success",
  "network": "testnet",
  "transaction_id": "at1...",
  "transaction_type": "execute",
  "execution_id": "...",
  "transaction": {},
  "broadcast": {
    "requested": true,
    "success": true,
    "endpoint": "https://api.explorer.provable.com/v2/testnet/transaction/broadcast"
  }
}
```

## Authorization Tool

Generate authorization payloads for testing:

```bash
# From local program file
cargo run --release --bin authorize -- \
  -f build/main.aleo \
  -F function_name \
  -k APrivateKey1... \
  -i "1u64" -i "aleo1..."

# From on-chain program
cargo run --release --bin authorize -- \
  -p program_name.aleo \
  -F function_name \
  -k APrivateKey1... \
  -i "1u64"
```

## Testing

```bash
cargo test
```

## License

Apache 2.0
