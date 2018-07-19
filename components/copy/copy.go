package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
)

type PortHandle struct {
	//Name string
	Writer *bufio.Writer
	Pipe   *os.File
}

func main() {
	// flag variables
	var brackets, control bool
	var packets, size bool
	var packetsPerFieldValue string
	// get configuration from flags
	unixfbp.DefFlags()
	flag.BoolVar(&brackets, "brackets", false, "expect bracketed input streams")
	flag.BoolVar(&control, "control", false, "count control packets as well")
	flag.BoolVar(&size, "size", false, "count size of packet bodies")
	flag.BoolVar(&packets, "packets", false, "count number of packets")
	flag.StringVar(&packetsPerFieldValue, "packetsperfieldvalue", "", "count number of packets per value of given header field")
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "ERROR: unexpected free arguments given")
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
		_, _, err = unixfbp.OpenOutPort(portName)
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
	for {
		// read frame
		frame, err = flowd.Deserialize(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// send it to given output ports
		for _, outPort := range unixfbp.OutPorts {
			//frame.Port = outPort
			if err = frame.Serialize(outPort.Writer); err != nil {
				//TODO handle EOF gracefully = remove from list of ouptorts and continue as usual; if len(outports) == 0 then break
				fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
			}
			// send it now (flush)
			//TODO optimize; only flush if no packets waiting on input
			if err = outPort.Writer.Flush(); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
			}
		}
	}
}
