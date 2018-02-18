package main

import (
	"bufio"
	"bytes"
	"fmt"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// open connection to network and display output
	netin := bufio.NewReader(os.Stdin)
	errout := bufio.NewWriter(os.Stderr)
	defer errout.Flush()

	// main loop
	var frame *flowd.Frame
	var err error

	for {
		// read frame
		frame, err = flowd.Deserialize(netin)
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
			fmt.Fprintln(errout, "<nil>")
		}
		if netin.Buffered() == 0 {
			if err = errout.Flush(); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: flushing errout:", err)
			}
		}
	}
}
