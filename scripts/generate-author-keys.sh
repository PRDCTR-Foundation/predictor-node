#!/usr/bin/env bash
set -euo pipefail
umask 077

AUTHOR_COUNT="${1:-}"
OUTPUT_FILE="${2:-authors.json}"

if [ -z "$AUTHOR_COUNT" ]; then
  echo "Usage: $0 <number-of-authors> [output-file]" >&2
  echo "Example: $0 5" >&2
  echo "Example: $0 5 authors.json" >&2
  exit 1
fi

if ! [[ "$AUTHOR_COUNT" =~ ^[0-9]+$ ]] || [ "$AUTHOR_COUNT" -lt 1 ]; then
  echo "Error: number-of-authors must be a positive integer" >&2
  exit 1
fi

if ! command -v subkey >/dev/null 2>&1; then
  echo "Error: subkey is not installed or not on PATH" >&2
  exit 1
fi

if ! command -v cast >/dev/null 2>&1; then
  echo "Error: cast is not installed or not on PATH" >&2
  echo >&2
  echo "Install Foundry with:" >&2
  echo "  curl -L https://foundry.paradigm.xyz | bash" >&2
  echo "  foundryup" >&2
  exit 1
fi

declare -a AUTHOR_ETH_ADDRESS
declare -a AUTHOR_ETH_PUBLIC_KEY
declare -a AUTHOR_T2_PUBLIC_KEY
declare -a AUTHOR_PUBLIC_JSON
declare -a AUTHOR_SECRET_OUTPUT

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

  KEY_SCHEME="$scheme"
  KEY_SECRET_PHRASE="$(sed -n 's/^Secret phrase: *//p' <<< "$output")"
  KEY_SECRET_SEED="$(sed -n 's/^  Secret seed: *//p' <<< "$output")"
  KEY_PUBLIC_KEY="$(sed -n 's/^  Public key (hex): *//p' <<< "$output")"
  KEY_ACCOUNT_ID="$(sed -n 's/^  Account ID: *//p' <<< "$output")"
  KEY_SS58_ADDRESS="$(sed -n 's/^  SS58 Address: *//p' <<< "$output")"

  if (
    [ -z "$KEY_SECRET_PHRASE" ] ||
    [ -z "$KEY_SECRET_SEED" ] ||
    [ -z "$KEY_PUBLIC_KEY" ] ||
    [ -z "$KEY_ACCOUNT_ID" ] ||
    [ -z "$KEY_SS58_ADDRESS" ]
  ); then
    echo "Error: failed to parse subkey output for scheme '$scheme'" >&2
    exit 1
  fi
}

generate_eth_key() {
  local output
  local eth_public_key_raw

  output="$(subkey generate --scheme ecdsa)"

  ETH_SECRET_PHRASE="$(sed -n 's/^Secret phrase: *//p' <<< "$output")"
  ETH_PRIVATE_KEY="$(sed -n 's/^  Secret seed: *//p' <<< "$output")"

  if [ -z "$ETH_SECRET_PHRASE" ] || [ -z "$ETH_PRIVATE_KEY" ]; then
    echo "Error: failed to parse Ethereum key from subkey output" >&2
    exit 1
  fi

  ETH_ADDRESS="$(cast wallet address "$ETH_PRIVATE_KEY")"
  eth_public_key_raw="$(cast wallet public-key --private-key "$ETH_PRIVATE_KEY")"

  # cast normally returns the uncompressed key without the 0x04 prefix.
  # Avoid duplicating the prefix if a future version includes it.
  eth_public_key_raw="${eth_public_key_raw#0x}"

  if [[ "$eth_public_key_raw" == 04* ]] && [ "${#eth_public_key_raw}" -eq 130 ]; then
    ETH_PUBLIC_KEY="0x${eth_public_key_raw}"
  else
    ETH_PUBLIC_KEY="0x04${eth_public_key_raw}"
  fi
}

substrate_public_json() {
  local indent="$1"

  printf '%s{\n' "$indent"
  printf '%s  "scheme": %s,\n' "$indent" "$(json_string "$KEY_SCHEME")"
  printf '%s  "publicKey": %s,\n' "$indent" "$(json_string "$KEY_PUBLIC_KEY")"
  printf '%s  "accountId": %s,\n' "$indent" "$(json_string "$KEY_ACCOUNT_ID")"
  printf '%s  "ss58Address": %s\n' "$indent" "$(json_string "$KEY_SS58_ADDRESS")"
  printf '%s}' "$indent"
}

eth_public_json() {
  local indent="$1"

  printf '%s{\n' "$indent"
  printf '%s  "scheme": "ecdsa / secp256k1 / ethereum",\n' "$indent"
  printf '%s  "address": %s,\n' "$indent" "$(json_string "$ETH_ADDRESS")"
  printf '%s  "uncompressedPublicKey": %s\n' \
    "$indent" \
    "$(json_string "$ETH_PUBLIC_KEY")"
  printf '%s}' "$indent"
}

append_substrate_secret() {
  local author_name="$1"
  local key_name="$2"

  printf -v SECRET_BLOCK \
    '%s\n  Scheme: %s\n  Secret phrase: %s\n  Secret seed: %s\n' \
    "$key_name" \
    "$KEY_SCHEME" \
    "$KEY_SECRET_PHRASE" \
    "$KEY_SECRET_SEED"

  AUTHOR_SECRET_OUTPUT[$author_name]+="$SECRET_BLOCK"
}

append_eth_secret() {
  local author_name="$1"

  printf -v SECRET_BLOCK \
    '%s\n  Scheme: ecdsa / secp256k1 / ethereum\n  Secret phrase: %s\n  Private key: %s\n  Address: %s\n' \
    "ethk" \
    "$ETH_SECRET_PHRASE" \
    "$ETH_PRIVATE_KEY" \
    "$ETH_ADDRESS"

  AUTHOR_SECRET_OUTPUT[$author_name]+="$SECRET_BLOCK"
}

GENERATED_AT="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

for ((i = 1; i <= AUTHOR_COUNT; i++)); do
  author_name="author-$i"
  AUTHOR_SECRET_OUTPUT[$i]=""

  generate_substrate_key "sr25519"
  account_json="$(substrate_public_json "        ")"
  append_substrate_secret "$i" "account"

  generate_substrate_key "sr25519"
  avnk_json="$(substrate_public_json "        ")"
  AUTHOR_T2_PUBLIC_KEY[$i]="$KEY_PUBLIC_KEY"
  append_substrate_secret "$i" "avnk"

  generate_substrate_key "sr25519"
  aura_json="$(substrate_public_json "        ")"
  append_substrate_secret "$i" "aura"

  generate_substrate_key "ed25519"
  gran_json="$(substrate_public_json "        ")"
  append_substrate_secret "$i" "gran"

  generate_substrate_key "sr25519"
  audi_json="$(substrate_public_json "        ")"
  append_substrate_secret "$i" "audi"

  generate_substrate_key "sr25519"
  imon_json="$(substrate_public_json "        ")"
  append_substrate_secret "$i" "imon"

  generate_eth_key
  AUTHOR_ETH_ADDRESS[$i]="$ETH_ADDRESS"
  AUTHOR_ETH_PUBLIC_KEY[$i]="$ETH_PUBLIC_KEY"
  ethk_json="$(eth_public_json "        ")"
  append_eth_secret "$i"

  AUTHOR_PUBLIC_JSON[$i]="$(cat <<EOF
    {
      "name": $(json_string "$author_name"),
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
done

{
  echo "{"
  printf '  "generatedAt": %s,\n' "$(json_string "$GENERATED_AT")"

  echo '  "authors": ['

  for ((i = 1; i <= AUTHOR_COUNT; i++)); do
    printf '%s' "${AUTHOR_PUBLIC_JSON[$i]}"

    if [ "$i" -lt "$AUTHOR_COUNT" ]; then
      echo ","
    else
      echo
    fi
  done

  echo '  ],'
  echo '  "bridgeConfig": ['

  for ((i = 1; i <= AUTHOR_COUNT; i++)); do
    echo '    {'
    printf '      "ethAddress": %s,\n' \
      "$(json_string "${AUTHOR_ETH_ADDRESS[$i]}")"
    printf '      "ethUncompressedPublicKey": %s,\n' \
      "$(json_string "${AUTHOR_ETH_PUBLIC_KEY[$i]}")"
    printf '      "t2PublicKey": %s\n' \
      "$(json_string "${AUTHOR_T2_PUBLIC_KEY[$i]}")"

    if [ "$i" -lt "$AUTHOR_COUNT" ]; then
      echo '    },'
    else
      echo '    }'
    fi
  done

  echo '  ]'
  echo '}'
} > "$OUTPUT_FILE"

chmod 600 "$OUTPUT_FILE"

echo
echo "============================================================"
echo "AUTHOR SECRETS — COPY THESE INTO THE SECURE LASTPASS NOTE"
echo "============================================================"

for ((i = 1; i <= AUTHOR_COUNT; i++)); do
  echo
  echo "------------------------------------------------------------"
  echo "author-$i"
  echo "------------------------------------------------------------"
  printf '%s' "${AUTHOR_SECRET_OUTPUT[$i]}"
done

echo
echo "============================================================"
echo "END OF AUTHOR SECRETS"
echo "============================================================"
echo
echo "Generated $AUTHOR_COUNT author accounts." >&2
echo "Public configuration written to: $OUTPUT_FILE" >&2
echo "The JSON file contains no private keys or secret phrases." >&2