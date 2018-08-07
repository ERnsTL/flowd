package main

import (
	"flag"
	"fmt"
	"os"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
)

func main() {
	// get configuration from argemunts = Unix IIP
	var delay time.Duration
	unixfbp.DefFlags()
	flag.DurationVar(&delay, "delay", 5*time.Second, "delay time")
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "ERROR: unexpected free arguments - exiting.")
		os.Exit(2)
	}
	if unixfbp.Debug {
		fmt.Fprintln(os.Stderr, "got delay time", delay)
	}
	// connect to FBP network
	var err error
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

	// main loop
	var frame *flowd.Frame
	for {
		// read frame
		frame, err = flowd.Deserialize(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			break
		}

		// sleep
		if unixfbp.Debug {
			fmt.Fprintln(os.Stderr, "got frame, delaying...")
		}
		time.Sleep(delay)
		if unixfbp.Debug {
			fmt.Fprintln(os.Stderr, "now forwarding.")
		}

		// send it to given output ports
		//frame.Port = "OUT"
		if err = frame.Serialize(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
		}
		if netin.Buffered() == 0 {
			if err = netout.Flush(); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
			}
		}
	}
}
