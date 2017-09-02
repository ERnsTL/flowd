package main

import (
	"bufio"
	"bytes"
	"fmt"
	"os"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
)

const maxFlushWait = 1 * time.Second // flush any buffered display output after at most this duration

func main() {
	netin := bufio.NewReader(os.Stdin)
	errout := bufio.NewWriter(os.Stderr)
	defer errout.Flush()

	// flush display output after x seconds if there is buffered data
	// NOTE: bufio.Writer.Write() flushes on its own if buffer is full
	go func() {
		for {
			time.Sleep(maxFlushWait)
			// NOTE: Flush() checks on its own if data buffered
			errout.Flush()
		}
	}()

	// main loop
	var frame *flowd.Frame
	var err error

	for {
		// read frame
		frame, err = flowd.ParseFrame(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, "ERROR:", err, "- exiting.")
			break
		}

		// check for closed input port
		if frame.Type == "control" && frame.BodyType == "PortClose" && frame.Port == "IN" {
			// shut down operations
			fmt.Fprintln(errout, "received port close notification - exiting.")
			break
		}

		// display frame body
		if frame.Body != nil {
			// output newline only if needed
			if bytes.HasSuffix(frame.Body, []byte{byte('\n')}) {
				fmt.Fprint(errout, string(frame.Body))
			} else {
				fmt.Fprintln(errout, string(frame.Body))
			}
		} else {
			fmt.Fprint(errout, "<nil>")
		}
	}
}
