package main

import (
	"fmt"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// read list of output ports
	outPorts := os.Args[1:]
	for _, outPort := range outPorts {
		fmt.Println("got output port:", outPort)
	}

	// pre-declare to reduce GC allocations
	var frame *flowd.Frame
	var err error

	for {
		// read frame
		frame, err = flowd.ParseFrame(os.Stdin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// send it
		for _, outPort := range outPorts {
			frame.Port = outPort
			frame.Marshal(os.Stdout) //TODO optimize, marshal only once
		}
	}
}
