#!/bin/bash

RPC=http://127.0.0.1:9988
# Must match the //Ferdie/N nodes in the local genesis preset
FERDIE_NODE_COUNT=5

for tool in subkey jq; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Error: $tool not found in PATH" >&2
    exit 1
  fi
done

insert() {  # $1 = suri (dev URI or 12 words)
  PUB=$(subkey inspect --scheme sr25519 --output-type json "$1" | jq -r .publicKey)
  echo "Injecting ${PUB} signing key to local setup."
  curl -s -H 'Content-Type: application/json' $RPC -d "{
    \"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"author_insertKey\",
    \"params\":[\"nodk\",\"$1\",\"$PUB\"]}"
}

# Ferdie is owner
for ((i = 0; i < FERDIE_NODE_COUNT; i++)); do
  insert "//Ferdie/$i"
done
# insert "word1 word2 ... word12"
