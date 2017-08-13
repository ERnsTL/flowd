package main

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"sort"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	fmt.Fprintln(os.Stderr, "reading packets")

	//TODO add configuration to do numeric sort
	//TODO maybe add sorting of substreams = bracketed groups

	var frame *flowd.Frame
	var err error
	var toSort []string

	// read frames
	for {
		frame, err = flowd.ParseFrame(netin)
		if err != nil {
			if err == io.EOF {
				break
			}
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}

		// check for port close notification
		if frame.Type == "control" && frame.BodyType == "PortClose" {
			fmt.Fprintln(os.Stderr, "got all messages:", len(toSort))
			// done
			break
		}

		// append to list
		toSort = append(toSort, string(frame.Body))
	}

	// sort
	fmt.Fprintln(os.Stderr, "sorting")
	sort.Strings(toSort)

	// send out sorted
	fmt.Fprintln(os.Stderr, "sending out sorted")
	outframe := flowd.Frame{
		Type:     "data",
		BodyType: "Sorted",
		Port:     "OUT",
	}
	for i := 0; i < len(toSort); i++ {
		outframe.Body = []byte(toSort[i])
		if err := outframe.Marshal(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
		}
	}

	// report
	fmt.Fprintln(os.Stderr, "all sent, exiting.")
}
