package main

import (
	"bufio"
	"fmt"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	//TODO seems unnecessary
	//netout := bufio.NewWriter(os.Stdout)
	//defer netout.Flush()

	var frame *flowd.Frame
	var err error

	for {
		// read frame
		frame, err = flowd.Deserialize(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// check for closed input port
		if frame.Type == "control" && frame.BodyType == "PortClose" && frame.Port == "IN" {
			// shut down operations
			fmt.Fprintln(os.Stderr, "received port close notification - exiting.")
			break
		}

		// discard frame
	}
}
