package main

import (
	"fmt"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// read list of output ports
	outPorts := os.Args[1:]
	if len(outPorts) == 0 {
		fmt.Println("ERROR: no output port names given")
		os.Exit(1)
	}

	var frame *flowd.Frame
	var err error

	for {
		// read frame
		frame, err = flowd.ParseFrame(os.Stdin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// send it to given output ports
		for _, outPort := range outPorts {
			//fmt.Fprint(os.Stderr, "copying to port", outPort)
			frame.Port = outPort
			frame.Marshal(os.Stdout) //TODO optimize, marshal only once
			//fmt.Fprintln(os.Stderr, "copied to port", outPort)
		}
	}
}
