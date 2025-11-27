#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 ]]; then
  cat <<'USAGE' >&2
Usage: authorize_call.sh <PROGRAM> <FUNCTION_NAME> <PRIVATE_KEY> [INPUT ...]

PROGRAM        Path to a compiled Aleo program file or an on-chain program ID (e.g. app.aleo)
FUNCTION_NAME  Name of the function to execute
PRIVATE_KEY    Aleo private key authorizing the execution
INPUT          Optional Leo literal inputs passed to the function (repeatable)

Example:
  ./scripts/authorize_call.sh build/main.aleo set_data_sgx APrivateKey1... \
      "1u64" "aleo1..."
  ./scripts/authorize_call.sh veru_oracle_v2.aleo set_data_sgx APrivateKey1...
USAGE
  exit 64
fi

PROGRAM_SPEC=$1
FUNCTION_NAME=$2
PRIVATE_KEY=$3
shift 3

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"

ARGS=(
  --function "$FUNCTION_NAME"
  --private-key "$PRIVATE_KEY"
)

if [[ -f "$PROGRAM_SPEC" ]]; then
  ARGS+=(--program-file "$PROGRAM_SPEC")
elif [[ -f "$REPO_ROOT/$PROGRAM_SPEC" ]]; then
  ARGS+=(--program-file "$REPO_ROOT/$PROGRAM_SPEC")
else
  ARGS+=(--program-id "$PROGRAM_SPEC")
  if [[ -n "${AUTHORIZE_NETWORK:-}" ]]; then
    ARGS+=(--network "${AUTHORIZE_NETWORK}")
  fi
  if [[ -n "${AUTHORIZE_EDITION:-}" ]]; then
    ARGS+=(--edition "${AUTHORIZE_EDITION}")
  fi
  if [[ -n "${AUTHORIZE_API_BASE:-}" ]]; then
    ARGS+=(--api-base "${AUTHORIZE_API_BASE}")
  fi
fi

for value in "$@"; do
  ARGS+=(--input "$value")
done

if [[ "${PRINT_ACCOUNT:-0}" != 0 ]]; then
  ARGS+=(--print-account)
fi

CARGO_CMD=(cargo run --manifest-path "$REPO_ROOT/Cargo.toml" --bin authorize --quiet --)

if [[ "${AUTHORIZE_RELEASE:-0}" != 0 ]]; then
  CARGO_CMD=(cargo run --manifest-path "$REPO_ROOT/Cargo.toml" --bin authorize --release --)
fi

"${CARGO_CMD[@]}" "${ARGS[@]}"
