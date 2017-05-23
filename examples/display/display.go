package main

import (
	"bufio"
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

		// check for closed input port
		if frame.Type == "control" && frame.BodyType == "PortClose" && frame.Port == "IN" {
			// shut down operations
			fmt.Fprintln(os.Stderr, "received port close notification - exiting.")
			break
		}

		// extract frame body
		if err != nil {
			fmt.Fprintln(os.Stderr, "ERROR:", err, "- exiting.")
			break
		}

		// display frame body
		if frame.Body != nil {
			fmt.Fprint(os.Stderr, string(frame.Body))
		} else {
			fmt.Fprint(os.Stderr, "<nil>")
		}
	}
}
