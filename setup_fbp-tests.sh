#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FBP_DIR="$ROOT_DIR/fbp-tests"

echo "Setting up fbp-tests environment in: $FBP_DIR"

mkdir -p "$FBP_DIR"
cd "$FBP_DIR"

# Ensure Volta-managed Node tools are available
export PATH="$HOME/.volta/bin:$PATH"
if ! command -v volta >/dev/null 2>&1; then
  echo "Error: 'volta' is not available in PATH ($PATH)" >&2
  echo "Install Volta first: https://volta.sh" >&2
  exit 1
fi

# Initialize npm project if missing
if [[ ! -f package.json ]]; then
  volta run npm init -y >/dev/null
fi

install_fbp_protocol() {
  volta run npm install --save-exact fbp-protocol@0.9.8
}

# Install/refresh protocol test suite dependency (with retries for transient network issues)
max_attempts=3
attempt=1
until install_fbp_protocol; do
  if [[ "$attempt" -ge "$max_attempts" ]]; then
    echo "Error: failed to install fbp-protocol after $max_attempts attempts" >&2
    exit 1
  fi
  echo "Install attempt $attempt failed; retrying in 3s..."
  sleep 3
  attempt=$((attempt + 1))
done

# Generate deterministic runtime test config for flowd
volta run npx fbp-init \
  --name flowd \
  --host localhost \
  --port 3569 \
  --collection core \
  --version 0.7

echo "fbp-tests setup complete."
echo "Next: run ./run_fbp-tests.sh from $ROOT_DIR"
