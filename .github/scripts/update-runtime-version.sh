#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cargo_toml="${repo_root}/Cargo.toml"
runtime_version_template="${repo_root}/runtime/src/runtime_version.rs.template"
runtime_version_rs="${repo_root}/runtime/src/runtime_version.rs"

version="$(
  sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "${cargo_toml}"
)"

if [[ ! "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Could not parse workspace version from ${cargo_toml}: ${version}" >&2
  exit 1
fi

IFS=. read -r major minor patch <<< "${version}"

for component in "${major}" "${minor}" "${patch}"; do
  if (( component > 99 )); then
    echo "Version component ${component} is too large for two-digit spec_version format" >&2
    exit 1
  fi
done

spec_version="$(printf "%02d_%02d_%02d_00" "${major}" "${minor}" "${patch}")"
sed "s/{{SPEC_VERSION}}/${spec_version}/g" "${runtime_version_template}" > "${runtime_version_rs}"
