#!/usr/bin/env bash
set -euo pipefail
umask 077

AUTHOR_COUNT="${1:-}"
OUTPUT_FILE="${2:-authors.keys}"

if [ -z "$AUTHOR_COUNT" ]; then
  echo "Usage: $0 <number-of-authors> [output-file]"
  echo "Example: $0 5"
  echo "Example: $0 5 authors.keys"
  exit 1
fi

if ! [[ "$AUTHOR_COUNT" =~ ^[0-9]+$ ]] || [ "$AUTHOR_COUNT" -lt 1 ]; then
  echo "Error: number-of-authors must be a positive integer"
  exit 1
fi

if ! command -v subkey >/dev/null 2>&1; then
  echo "Error: subkey is not installed or not on PATH"
  exit 1
fi

if ! command -v cast >/dev/null 2>&1; then
  echo "Error: cast is not installed or not on PATH"
  echo
  echo "Install Foundry with:"
  echo "  curl -L https://foundry.paradigm.xyz | bash"
  echo "  foundryup"
  exit 1
fi

declare -a AUTHOR_ETH_ADDRESS
declare -a AUTHOR_ETH_PUBLIC_KEY
declare -a AUTHOR_T2_PUBLIC_KEY

json_string() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  printf '"%s"' "$value"
}

generate_substrate_key() {
  local label="$1"
  local scheme="$2"

  local output
  output="$(subkey generate --scheme "$scheme")"

  KEY_LABEL="$label"
  KEY_SCHEME="$scheme"
  KEY_SECRET_PHRASE="$(echo "$output" | sed -n 's/^Secret phrase: *//p')"
  KEY_SECRET_SEED="$(echo "$output" | sed -n 's/^  Secret seed: *//p')"
  KEY_PUBLIC_KEY="$(echo "$output" | sed -n 's/^  Public key (hex): *//p')"
  KEY_ACCOUNT_ID="$(echo "$output" | sed -n 's/^  Account ID: *//p')"
  KEY_SS58_ADDRESS="$(echo "$output" | sed -n 's/^  SS58 Address: *//p')"
}

generate_eth_key() {
  local output
  output="$(subkey generate --scheme ecdsa)"

  ETH_PRIVATE_KEY="$(echo "$output" | sed -n 's/^  Secret seed: *//p')"
  ETH_ADDRESS="$(cast wallet address "$ETH_PRIVATE_KEY")"

  local eth_public_key_raw
  eth_public_key_raw="$(cast wallet public-key --private-key "$ETH_PRIVATE_KEY")"
  ETH_PUBLIC_KEY="0x04${eth_public_key_raw#0x}"
}

write_substrate_key_json() {
  local indent="$1"
  local key_name="$2"
  local trailing_comma="${3:-true}"

  printf '%s"%s": {\n' "$indent" "$key_name"
  printf '%s  "scheme": %s,\n' "$indent" "$(json_string "$KEY_SCHEME")"
  printf '%s  "secretPhrase": %s,\n' "$indent" "$(json_string "$KEY_SECRET_PHRASE")"
  printf '%s  "secretSeed": %s,\n' "$indent" "$(json_string "$KEY_SECRET_SEED")"
  printf '%s  "publicKey": %s,\n' "$indent" "$(json_string "$KEY_PUBLIC_KEY")"
  printf '%s  "accountId": %s,\n' "$indent" "$(json_string "$KEY_ACCOUNT_ID")"
  printf '%s  "ss58Address": %s\n' "$indent" "$(json_string "$KEY_SS58_ADDRESS")"

  if [ "$trailing_comma" = "true" ]; then
    printf '%s},\n' "$indent"
  else
    printf '%s}\n' "$indent"
  fi
}

write_eth_key_json() {
  local indent="$1"
  local trailing_comma="${2:-true}"

  printf '%s"ethk": {\n' "$indent"
  printf '%s  "scheme": "ecdsa / secp256k1 / ethereum",\n' "$indent"
  printf '%s  "privateKey": %s,\n' "$indent" "$(json_string "$ETH_PRIVATE_KEY")"
  printf '%s  "address": %s,\n' "$indent" "$(json_string "$ETH_ADDRESS")"
  printf '%s  "uncompressedPublicKey": %s\n' "$indent" "$(json_string "$ETH_PUBLIC_KEY")"

  if [ "$trailing_comma" = "true" ]; then
    printf '%s},\n' "$indent"
  else
    printf '%s}\n' "$indent"
  fi
}

GENERATED_AT="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

{
  echo "{"
  printf '  "generatedAt": %s,\n' "$(json_string "$GENERATED_AT")"

  echo '  "sudo": {'
  generate_substrate_key "account" "sr25519"
  write_substrate_key_json "    " "account" "false"
  echo '  },'

  echo '  "authors": ['

  for i in $(seq 1 "$AUTHOR_COUNT"); do
    echo '    {'
    printf '      "name": %s,\n' "$(json_string "author-$i")"

    generate_substrate_key "account" "sr25519"
    write_substrate_key_json "      " "account" "true"

    generate_substrate_key "avnk" "sr25519"
    AUTHOR_T2_PUBLIC_KEY[$i]="$KEY_PUBLIC_KEY"
    write_substrate_key_json "      " "avnk" "true"

    generate_substrate_key "aura" "sr25519"
    write_substrate_key_json "      " "aura" "true"

    generate_substrate_key "gran" "ed25519"
    write_substrate_key_json "      " "gran" "true"

    generate_substrate_key "audi" "sr25519"
    write_substrate_key_json "      " "audi" "true"

    generate_substrate_key "imon" "sr25519"
    write_substrate_key_json "      " "imon" "true"

    generate_eth_key
    AUTHOR_ETH_ADDRESS[$i]="$ETH_ADDRESS"
    AUTHOR_ETH_PUBLIC_KEY[$i]="$ETH_PUBLIC_KEY"
    write_eth_key_json "      " "false"

    if [ "$i" -eq "$AUTHOR_COUNT" ]; then
      echo '    }'
    else
      echo '    },'
    fi
  done

  echo '  ],'

  echo '  "bridgeConfig": {'
  echo '    "authors": ['

  for i in $(seq 1 "$AUTHOR_COUNT"); do
    echo '      {'
    printf '        "ethAddress": %s,\n' "$(json_string "${AUTHOR_ETH_ADDRESS[$i]}")"
    printf '        "ethUncompressedPublicKey": %s,\n' "$(json_string "${AUTHOR_ETH_PUBLIC_KEY[$i]}")"
    printf '        "t2PublicKey": %s\n' "$(json_string "${AUTHOR_T2_PUBLIC_KEY[$i]}")"

    if [ "$i" -eq "$AUTHOR_COUNT" ]; then
      echo '      }'
    else
      echo '      },'
    fi
  done

  echo '    ]'
  echo '  }'
  echo '}'
} > "$OUTPUT_FILE"

chmod 600 "$OUTPUT_FILE"

echo "Generated SUDO plus $AUTHOR_COUNT Author key bundles"
echo "Output written to: $OUTPUT_FILE"
echo "IMPORTANT: For DEV/TESTNET usage only. $OUTPUT_FILE contains private keys and seed phrases. Do not commit it."