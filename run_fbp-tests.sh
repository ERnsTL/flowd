#!/bin/bash

set -e

echo "Building flowd..."
cargo build --release

if timeout 1 bash -c 'echo > /dev/tcp/localhost/3569' 2>/dev/null; then
    echo "Error: port 3569 is already in use. Stop the existing process and retry."
    exit 1
fi

echo "Starting flowd..."

# Start flowd in background
./target/release/flowd-rs &
FLOWD_PID=$!

# Function to cleanup
cleanup() {
    echo "Stopping flowd..."
    kill $FLOWD_PID 2>/dev/null || true
}

trap cleanup EXIT

sleep 1
if ! kill -0 "$FLOWD_PID" 2>/dev/null; then
    echo "Error: flowd exited before becoming ready."
    exit 1
fi

# Wait for port 3569 to be open
echo "Waiting for flowd to be ready on port 3569..."
timeout 30 bash -c 'until echo > /dev/tcp/localhost/3569; do sleep 1; done'

echo "flowd is ready. Running FBP protocol tests..."

# Change to fbp-tests directory
cd fbp-tests

# Set PATH for volta
export PATH="$HOME/.volta/bin:$PATH"

# Run the tests
volta run npx fbp-test

echo "FBP tests completed."
