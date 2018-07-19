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
	unixfbp.DefFlags() //TODO ordering of ports is currently not guaranteed -> need to give ordering as free arguments
	flag.Parse()
	if flag.NArg() == 0 {
		fmt.Fprintln(os.Stderr, "ERROR: missing port order in free arguments")
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	//TODO check if all ports from list are also declared
	if unixfbp.Debug {
		fmt.Printf("got %d inports: %v\n", len(unixfbp.InPorts), flag.Args())
	}
	// connect to FBP network
	netout, _, err := unixfbp.OpenOutPort("OUT")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	defer netout.Flush()

	// prepare variables
	var frame *flowd.Frame

	// for each specified input port...
	for _, portName := range flag.Args() {
		// open that inport
		if unixfbp.Debug {
			fmt.Println("draining input port", portName)
		}
		netin, inPipe, err := unixfbp.OpenInPort(portName)
		if err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: opening input port:", err, "- exiting.")
			os.Exit(1)
		}
		// read new frames
		for {
			// read frame
			frame, err = flowd.Deserialize(netin)
			if err != nil {
				fmt.Fprintln(os.Stderr, err)
				err = inPipe.Close()
				if err != nil {
					fmt.Fprintf(os.Stderr, "ERROR: closing input port %s: %s\n", portName, err)
				}
				break
			}

			// send it to output port
			if unixfbp.Debug {
				fmt.Println("forwarding packet")
			}
			//frame.Port = "OUT"
			if err := frame.Serialize(netout); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
				break
			}
		}
	}

	// report
	fmt.Fprintln(os.Stderr, "exiting.")
}
