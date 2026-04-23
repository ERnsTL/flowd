# Shutdown and Signal Handling

This document describes how flowd handles shutdown signals and graceful termination.

## Overview

flowd supports graceful shutdown on signals to ensure running networks are properly stopped and resources cleaned up. Behavior differs based on the signal received to accommodate different shutdown contexts (systemd vs. user interrupt).

## Signal Behaviors

### SIGTERM (Graceful Shutdown, e.g., systemd stop)

- **Trigger:** Sent by systemd when `systemctl stop` is issued.
- **Behavior:**
  - Persists the current graph state to disk.
  - Attempts graceful network shutdown with timeout (internal thread join timeouts apply).
  - Logs progress: "Signal received, waiting up to Xs for network shutdown..."
  - If successful, logs "Network stopped gracefully" and exits.
  - If failed/timed out, logs warning and exits hard.
- **Purpose:** Planned shutdown, preserve state for restart.

### SIGINT (User Interrupt, e.g., Ctrl+C)

- **Trigger:** Sent by terminal on Ctrl+C or kill -INT.
- **Behavior:**
  - No persistence (avoids unwanted saves during testing/debugging).
  - Attempts graceful network shutdown with timeout.
  - Same logging and timeout behavior as SIGTERM.
- **Purpose:** User interruption, quick exit without saving.

### SIGKILL (Forced Termination)

- **Trigger:** Sent by systemd after SIGTERM timeout (default 90s) or `kill -9`.
- **Behavior:** Cannot be caught—kernel forces immediate termination. No cleanup or persistence.
- **Purpose:** Last resort to stop unresponsive processes.

## Implementation Details

- Signal handlers use `signal-hook` crate to register for SIGTERM and SIGINT.
- Handlers set atomic flags to trigger shutdown in the main server loop.
- Shutdown code in `FlowdServer::start()` checks flags and performs signal-specific actions (persist on SIGTERM).
- Network stop uses `Runtime::stop()` with internal timeouts for thread joins and process signaling.
- Persistence calls `Runtime::persist()` to save graph definitions.

## Configuration

- Systemd timeout: Configurable via `TimeoutStopSec` in unit files (default 90s).
- No flowd-specific config—behavior is hardcoded per signal.

## See Also

- [Runtime Protocol](fbp_format.md) for network control commands.
- Code: `src/server.rs` (signal registration), `src/lib.rs` (stop/persist logic).