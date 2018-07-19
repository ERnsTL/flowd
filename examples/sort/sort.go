package main

import (
	"flag"
	"fmt"
	"io"
	"os"
	"sort"

	"github.com/ERnsTL/UnixFBP/libunixfbp"
	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// get configuration from argemunts = Unix IIP
	//TODO add configuration to do numeric sort
	//TODO maybe add sorting of substreams = bracketed groups
	unixfbp.DefFlags()
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "ERROR: unexpected free argument(s) given")
		os.Exit(2)
	}
	// connect to FBP network
	netin, _, err := unixfbp.OpenInPort("IN")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	netout, _, err := unixfbp.OpenOutPort("OUT")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	if !unixfbp.Quiet {
		fmt.Fprintln(os.Stderr, "reading packets")
	}

	// prepare variables
	var frame *flowd.Frame
	var toSort []string

	// read frames to sort
	for {
		frame, err = flowd.Deserialize(netin)
		if err != nil {
			if err == io.EOF {
				fmt.Fprintln(os.Stderr, "got all messages:", len(toSort))
				break
			}
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}

		// append to list
		toSort = append(toSort, string(frame.Body))
	}

	// sort
	if !unixfbp.Quiet {
		fmt.Fprintln(os.Stderr, "sorting")
	}
	sort.Strings(toSort)

	// send out sorted
	if !unixfbp.Quiet {
		fmt.Fprintln(os.Stderr, "sending out sorted")
	}
	outframe := flowd.Frame{
		Type:     "data",
		BodyType: "Sorted",
		//Port:     "OUT",
	}
	for i := 0; i < len(toSort); i++ {
		outframe.Body = []byte(toSort[i])
		if err := outframe.Serialize(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: serializing frame:", err.Error())
		}
	}

	// report
	if !unixfbp.Quiet {
		fmt.Fprintln(os.Stderr, "all sent, exiting.")
	}
}
