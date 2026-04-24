
## Testing

The code now includes detailed test scenarios covering all requested areas:

- __Non-blocking subprocess spawning__ - Verify process() returns quickly
- __Signal responsiveness__ - Ping/pong during long-running subprocesses
- __Stop signal cleanup__ - Proper termination and resource cleanup
- __Various subprocess behaviors__ - Fast exit, slow output, hanging processes
- __Budget respect__ - Component honors execution limits
- __Configuration loading__ - Lazy initialization from ports
- __EOF handling__ - Proper shutdown on input abandonment

### Implementation Notes:

- Tests instrument the runtime+scheduler to test for actual whole-system behavior
- Subprocess testing needs careful timeout handling to prevent flaky tests
- The component maintains all original functionality while being scheduler-compatible
