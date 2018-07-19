package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
)

func main() {
	// get configuration from argemunts = Unix IIP
	unixfbp.DefFlags()
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "ERROR: unexpected free argument(s)")
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	// checks
	if len(unixfbp.OutPorts) > 1 {
		fmt.Fprintln(os.Stderr, "ERROR: only one input port supported at the moment")
		os.Exit(2)
	}
	// connect to FBP network
	netin, _, err := unixfbp.OpenInPort("IN")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}

	// main loop
	for {
		// read frame
		_, err = flowd.Deserialize(netin)
		if err != nil {
			//TODO check for EOF
			fmt.Fprintln(os.Stderr, err)
			break
		}

		// discard frame
	}
}
