package main

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
)

const delayDuration = 1 * time.Millisecond

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()

	// prepare variables
	var frame *flowd.Frame //TODO why is this pointer to Frame?
	var err error
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
			if err = outframe.Marshal(netout); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err)
			}
		}

		// check for error
		if scanner.Err() != nil {
			fmt.Fprintln(os.Stderr, "ERROR: scanning for line: "+scanner.Err().Error())
			os.Exit(1)
		} else {
			fmt.Fprintf(os.Stderr, "finished at %d lines total\n", linecount)
		}

		// send done notification
		close(scannerDone)
	}()

	// read data from FBP network, hand it over to scanner buffer
	for {
		// read frame
		frame, err = flowd.ParseFrame(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// check for closed input port
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
				if err = frame.Marshal(netout); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err)
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
