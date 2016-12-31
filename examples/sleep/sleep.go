package main

import (
	"bufio"
	"fmt"
	"os"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// get configuration from IIP = initial information packet/frame
	var delay time.Duration
	fmt.Fprintln(os.Stderr, "wait for IIP")
	bufr := bufio.NewReader(os.Stdin)
	if iip, err := flowd.GetIIP("CONF", bufr); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		if len(iip) == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: no delay time given in IIP, format is [duration]")
			os.Exit(1)
		} else if delay, err = time.ParseDuration(iip); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: malformed delay time given in IIP, format is [duration], eg. 5s")
			os.Exit(1)
		}
	}
	fmt.Fprintln(os.Stderr, "got delay time", delay)

	var frame *flowd.Frame //TODO why is this pointer to frame?
	var err error

	for {
		// read frame
		frame, err = flowd.ParseFrame(bufr)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// sleep
		fmt.Fprintln(os.Stderr, "got frame, delaying...")
		time.Sleep(delay)
		fmt.Fprintln(os.Stderr, "now forwarding.")

		// send it to given output ports
		frame.Port = "OUT"
		if err := frame.Marshal(os.Stdout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
		}
	}
}