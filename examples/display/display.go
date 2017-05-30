package main

import (
	"bufio"
	"bytes"
	"fmt"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	var frame *flowd.Frame //TODO why is this pointer of Frame?
	var err error
	bufr := bufio.NewReader(os.Stdin)

	for {
		// read frame
		frame, err = flowd.ParseFrame(bufr)
		if err != nil {
			fmt.Fprintln(os.Stderr, "ERROR:", err, "- exiting.")
			break
		}

		// check for closed input port
		if frame.Type == "control" && frame.BodyType == "PortClose" && frame.Port == "IN" {
			// shut down operations
			fmt.Fprintln(os.Stderr, "received port close notification - exiting.")
			break
		}

		// display frame body
		if frame.Body != nil {
			// output newline only if needed
			if bytes.HasSuffix(frame.Body, []byte{byte('\n')}) {
				fmt.Fprint(os.Stderr, string(frame.Body))
			} else {
				fmt.Fprintln(os.Stderr, string(frame.Body))
			}
		} else {
			fmt.Fprint(os.Stderr, "<nil>")
		}
	}
}
