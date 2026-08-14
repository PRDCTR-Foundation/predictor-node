#!/usr/bin/env bash
set -euo pipefail
umask 077

AUTHOR_COUNT="${1:-}"
OUTPUT_FILE="${2:-authors.json}"

usage() {
  echo "Usage: $0 <number-of-authors> [output-file]" >&2
  echo "Example: $0 5 authors.json" >&2
  exit 1
}

[[ "$AUTHOR_COUNT" =~ ^[1-9][0-9]*$ ]] || usage

for command in subkey cast; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Error: '$command' is not installed or not on PATH" >&2
    exit 1
  }
done

declare -a AUTHOR_JSON
declare -a AUTHOR_FULL_JSON
declare -a ETH_ADDRESSES
declare -a ETH_PUBLIC_KEYS
declare -a T2_PUBLIC_KEYS

json_string() {
  local value="$1"

  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\n'/\\n}"
  value="${value//$'\r'/\\r}"
  value="${value//$'\t'/\\t}"

  printf '"%s"' "$value"
}

generate_substrate_key() {
  local scheme="$1"
  local output

  output="$(subkey generate --scheme "$scheme")"

  SCHEME="$scheme"
  SECRET_PHRASE="$(sed -n 's/^Secret phrase:[[:space:]]*//p' <<< "$output")"
  SECRET_SEED="$(sed -n 's/^[[:space:]]*Secret seed:[[:space:]]*//p' <<< "$output")"
  PUBLIC_KEY="$(sed -n 's/^[[:space:]]*Public key (hex):[[:space:]]*//p' <<< "$output")"
  ACCOUNT_ID="$(sed -n 's/^[[:space:]]*Account ID:[[:space:]]*//p' <<< "$output")"
  SS58_ADDRESS="$(sed -n 's/^[[:space:]]*SS58 Address:[[:space:]]*//p' <<< "$output")"

  [[ -n "$SECRET_PHRASE" &&
     -n "$SECRET_SEED" &&
     -n "$PUBLIC_KEY" &&
     -n "$ACCOUNT_ID" &&
     -n "$SS58_ADDRESS" ]] || {
    echo "Error: failed to parse $scheme key output" >&2
    exit 1
  }
}

generate_eth_key() {
  local output public_key

  output="$(subkey generate --scheme ecdsa)"

  ETH_SECRET_PHRASE="$(sed -n 's/^Secret phrase:[[:space:]]*//p' <<< "$output")"
  ETH_PRIVATE_KEY="$(sed -n 's/^[[:space:]]*Secret seed:[[:space:]]*//p' <<< "$output")"
  ETH_COMPRESSED_PUBLIC_KEY="$(sed -n 's/^[[:space:]]*Public key (hex):[[:space:]]*//p' <<< "$output")"

  [[ -n "$ETH_SECRET_PHRASE" && -n "$ETH_PRIVATE_KEY" ]] || {
    echo "Error: failed to parse Ethereum key output" >&2
    exit 1
  }

  ETH_ADDRESS="$(cast wallet address "$ETH_PRIVATE_KEY")"
  public_key="$(cast wallet public-key --private-key "$ETH_PRIVATE_KEY")"
  public_key="${public_key#0x}"

  if [[ "$public_key" == 04* && ${#public_key} -eq 130 ]]; then
    ETH_PUBLIC_KEY="0x$public_key"
  else
    ETH_PUBLIC_KEY="0x04$public_key"
  fi
}

substrate_json() {
  cat <<EOF
{
          "scheme": $(json_string "$SCHEME"),
          "accountId": $(json_string "$ACCOUNT_ID"),
          "ss58Address": $(json_string "$SS58_ADDRESS")
        }
EOF
}

substrate_full_json() {
  cat <<EOF
{
          "scheme": $(json_string "$SCHEME"),
          "accountId": $(json_string "$ACCOUNT_ID"),
          "ss58Address": $(json_string "$SS58_ADDRESS"),
          "publicKey": $(json_string "$PUBLIC_KEY"),
          "secretPhrase": $(json_string "$SECRET_PHRASE"),
          "secretSeed": $(json_string "$SECRET_SEED")
        }
EOF
}

eth_json() {
  cat <<EOF
{
          "scheme": "ecdsa / secp256k1 / ethereum",
          "address": $(json_string "$ETH_ADDRESS"),
          "compressedPublicKey": $(json_string "$ETH_COMPRESSED_PUBLIC_KEY"),
          "uncompressedPublicKey": $(json_string "$ETH_PUBLIC_KEY")
        }
EOF
}

eth_full_json() {
  cat <<EOF
{
          "scheme": "ecdsa / secp256k1 / ethereum",
          "address": $(json_string "$ETH_ADDRESS"),
          "compressedPublicKey": $(json_string "$ETH_COMPRESSED_PUBLIC_KEY"),
          "uncompressedPublicKey": $(json_string "$ETH_PUBLIC_KEY"),
          "secretPhrase": $(json_string "$ETH_SECRET_PHRASE"),
          "privateKey": $(json_string "$ETH_PRIVATE_KEY")
        }
EOF
}

for ((i = 1; i <= AUTHOR_COUNT; i++)); do
  generate_substrate_key sr25519
  account_json="$(substrate_json)"
  account_full_json="$(substrate_full_json)"

  generate_substrate_key sr25519
  avnk_json="$(substrate_json)"
  avnk_full_json="$(substrate_full_json)"
  T2_PUBLIC_KEYS[$i]="$PUBLIC_KEY"

  generate_substrate_key sr25519
  aura_json="$(substrate_json)"
  aura_full_json="$(substrate_full_json)"

  generate_substrate_key ed25519
  gran_json="$(substrate_json)"
  gran_full_json="$(substrate_full_json)"

  generate_substrate_key sr25519
  audi_json="$(substrate_json)"
  audi_full_json="$(substrate_full_json)"

  generate_substrate_key sr25519
  imon_json="$(substrate_json)"
  imon_full_json="$(substrate_full_json)"

  generate_eth_key
  ethk_json="$(eth_json)"
  ethk_full_json="$(eth_full_json)"
  ETH_ADDRESSES[$i]="$ETH_ADDRESS"
  ETH_PUBLIC_KEYS[$i]="$ETH_PUBLIC_KEY"

  AUTHOR_JSON[$i]="$(cat <<EOF
    {
      "name": "author-$i",
      "account": $account_json,
      "avnk": $avnk_json,
      "aura": $aura_json,
      "gran": $gran_json,
      "audi": $audi_json,
      "imon": $imon_json,
      "ethk": $ethk_json
    }
EOF
)"

  AUTHOR_FULL_JSON[$i]="$(cat <<EOF
    {
      "name": "author-$i",
      "account": $account_full_json,
      "avnk": $avnk_full_json,
      "aura": $aura_full_json,
      "gran": $gran_full_json,
      "audi": $audi_full_json,
      "imon": $imon_full_json,
      "ethk": $ethk_full_json
    }
EOF
)"
done

{
  echo '{'
  printf '  "generatedAt": %s,\n' \
    "$(json_string "$(date -u '+%Y-%m-%dT%H:%M:%SZ')")"

  echo '  "authors": ['

  for ((i = 1; i <= AUTHOR_COUNT; i++)); do
    printf '%s' "${AUTHOR_JSON[$i]}"
    ((i < AUTHOR_COUNT)) && echo ',' || echo
  done

  echo '  ],'
  echo '  "bridgeConfig": ['

  for ((i = 1; i <= AUTHOR_COUNT; i++)); do
    cat <<EOF
    {
      "ethAddress": $(json_string "${ETH_ADDRESSES[$i]}"),
      "ethUncompressedPublicKey": $(json_string "${ETH_PUBLIC_KEYS[$i]}"),
      "t2PublicKey": $(json_string "${T2_PUBLIC_KEYS[$i]}")
    }
EOF
    ((i < AUTHOR_COUNT)) && echo ',' || echo
  done

  echo '  ]'
  echo '}'
} > "$OUTPUT_FILE"

chmod 600 "$OUTPUT_FILE"

{
  echo '{'
  printf '  "generatedAt": %s,\n' \
    "$(json_string "$(date -u '+%Y-%m-%dT%H:%M:%SZ')")"

  echo '  "authors": ['

  for ((i = 1; i <= AUTHOR_COUNT; i++)); do
    printf '%s' "${AUTHOR_FULL_JSON[$i]}"
    ((i < AUTHOR_COUNT)) && echo ',' || echo
  done

  echo '  ]'
  echo '}'
}

echo "Generated $AUTHOR_COUNT author account(s)." >&2
echo "Public configuration (no private keys or secret phrases) written to: $OUTPUT_FILE" >&2
echo "Private key material (secret phrases, seeds, private keys) was printed to stdout as JSON — store it securely." >&2