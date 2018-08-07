package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
)

func main() {
	// get configuration from flags = Unix IIP (which can be generated out of another FIFO or similar)
	unixfbp.DefFlags()
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "ERROR: unexpected free argument(s) found")
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	// open network connections
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
	defer netout.Flush()

	// prepare variables
	var frame *flowd.Frame //TODO why is this pointer to Frame?
	//var err error
	scannerDone := make(chan struct{})
	piper, pipew := io.Pipe()

	// start scanning for lines in buffer
	go func() {
		// prepare variables
		scanner := bufio.NewScanner(piper)
		outframe := &flowd.Frame{
			Type:     "data",
			BodyType: "LineData",
			Port:     "OUT",
		}

		// scan loop
		linecount := 0
		for scanner.Scan() {
			// collect line
			outframe.Body = scanner.Bytes()
			linecount++
			// send it to FBP network
			if err = outframe.Serialize(netout); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err)
				os.Exit(1)
			}
		}

		// check for error
		if scanner.Err() != nil {
			fmt.Fprintln(os.Stderr, "ERROR: scanning for line: "+scanner.Err().Error())
			os.Exit(1)
		} else if !unixfbp.Quiet {
			fmt.Fprintf(os.Stderr, "finished at %d lines total\n", linecount)
		}

		// send done notification
		close(scannerDone)
	}()

	// read data from FBP network, hand it over to scanner buffer
	if !unixfbp.Quiet {
		fmt.Println("splitting")
	}
	for {
		// read frame
		frame, err = flowd.Deserialize(netin)
		if err != nil {
			if err == io.EOF {
				fmt.Println("EOF on input - finishing up.")
				pipew.Close()
				break
			}
			fmt.Fprintln(os.Stderr, err)
			break
		}

		// check for closed input port
		//TODO not neccessary any more
		if frame.Type == "control" && frame.BodyType == "PortClose" && frame.Port == "IN" {
			// shut down operations
			fmt.Fprintln(os.Stderr, "input port closed - finishing up.")
			pipew.Close()
			break
		}

		if frame.Type == "control" {
			switch frame.BodyType {
			case "BracketClose":
				// finish remaining data as line if still data in buffer
				_ = pipew.Close()
				// make new pipe
				piper, pipew = io.Pipe()
				// forward bracket
				fallthrough
			case "BracketOpen":
				// forward bracket
				frame.Port = "OUT"
				if err = frame.Serialize(netout); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err)
					os.Exit(1)
				}
			}
		} else {
			// write frame body into buffer to be scanned
			if _, err = pipew.Write(frame.Body); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: writing received frame body into pipe to scanner")
				os.Exit(1)
			}
		}
	}

	// wait for scanner Goroutine to finish
	<-scannerDone
	fmt.Fprintln(os.Stderr, "exiting.")
}
