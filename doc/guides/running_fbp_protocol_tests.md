# Running FBP Protocol Tests

This guide explains how to run the official Flow-Based Programming (FBP) protocol test suite against flowd to validate protocol compliance.

## Overview

flowd implements the FBP protocol over WebSocket. The official test suite (`fbp-protocol`) can be used to verify that flowd correctly implements the protocol.

## Prerequisites

- Rust and Cargo installed
- Volta (Node.js version manager) installed: `curl https://get.volta.sh | bash`

## Local Testing

### Setup

1. **Build flowd:**
   ```bash
   cargo build --release
   ```

2. **Install Node.js via Volta:**
   ```bash
   export PATH="$HOME/.volta/bin:$PATH"
   volta install node
   ```

3. **Set up FBP test environment:**
   ```bash
   ./setup_fbp-tests.sh
   ```

### Run Tests

Execute the test runner script:
```bash
./run_fbp-tests.sh
```

This script will:
- Start flowd in the background
- Wait for the WebSocket endpoint to be ready (port 3569)
- Run the FBP protocol test suite
- Stop flowd
- Exit with the test result (0 = pass, non-zero = fail)

## CI Testing

The FBP protocol tests run automatically in CI on every push and pull request via the `.github/workflows/fbp-protocol-tests.yml` workflow.

The CI:
- Builds flowd
- Sets up the Node.js test environment
- Runs the same `./run_fbp-tests.sh` script
- Fails the build if any test fails

## Test Output

Test results are displayed in the console output. The test suite provides detailed pass/fail information for each protocol aspect tested.

## Troubleshooting

- Ensure port 3569 is not in use by other processes
- Check that flowd builds successfully
- Verify Node.js and npm are properly installed via Volta

## Protocol Compliance

Passing these tests ensures flowd correctly implements the FBP protocol for:
- Runtime discovery
- Graph management
- Component introspection
- Network execution control
- Message passing semantics
