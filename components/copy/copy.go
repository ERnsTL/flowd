package main

import (
	"flag"
	"fmt"
	"os"
	"io"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
)

func main() {
	// flag variables
	// get configuration from flags
	unixfbp.DefFlags()
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "ERROR: unexpected free arguments given (outports are auto-detected)")
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	if len(unixfbp.OutPorts) == 0 {
		fmt.Println("ERROR: no output ports given")
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	// connect to the network
	var err error
	netin, _, err := unixfbp.OpenInPort("IN")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	// NOTE: is backed by an array internally for small number of entries
	for portName, outPort := range unixfbp.OutPorts {
		// NOTE: re-assigning to avoid nil pointer access on defer (TODO optimize)
		outPort.Writer, _, err = unixfbp.OpenOutPort(portName)
		if err != nil {
			fmt.Println("ERROR:", err)
			os.Exit(2)
		}
		defer outPort.Writer.Flush()
	}
	if !unixfbp.Quiet {
		fmt.Fprintln(os.Stderr, "got output ports", unixfbp.OutPorts)
	}

	// main loop
	var frame *flowd.Frame
	var buffered int
	for {
		// read frame
		frame, err = flowd.Deserialize(netin)
		if err != nil {
			if err == io.EOF {
				if !unixfbp.Quiet {
					fmt.Fprintln(os.Stderr, "EOF - exiting")
				}
				break
			}
			fmt.Fprintln(os.Stderr, err)
			break
		}

		// send it to given output ports
		buffered = netin.Buffered()
		for _, outPort := range unixfbp.OutPorts {
			//frame.Port = outPort
			if err = frame.Serialize(outPort.Writer); err != nil {
				//TODO handle EOF gracefully = remove from list of ouptorts and continue as usual; if len(outports) == 0 then break
				fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
			}
			// only flush if no packets waiting on input
			if buffered == 0 {
				// send it now (flush)
				if err = outPort.Writer.Flush(); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
				}
			}
		}
	}
}
