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

		// extract frame body
		if err != nil {
			fmt.Fprintln(os.Stderr, "ERROR:", err, "- exiting.")
			break
		}

		// display frame body
		if frame.Body != nil {
			fmt.Fprintln(os.Stderr, string(frame.Body))
		} else {
			fmt.Fprintln(os.Stderr, "<nil>")
		}
	}
}
