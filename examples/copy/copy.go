package main

import (
	"bufio"
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
	bufr := bufio.NewReader(os.Stdin)

	for {
		// read frame
		frame, err = flowd.ParseFrame(bufr)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// send it to given output ports
		for _, outPort := range outPorts {
			frame.Port = outPort
			//TODO optimize, marshal frame only once
			if err := frame.Marshal(os.Stdout); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
			}
		}
	}
}
