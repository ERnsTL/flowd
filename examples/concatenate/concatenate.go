package main

import (
	"bufio"
	"fmt"
	"os"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// get configuration from IIP = initial information packet/frame
	var inPorts []string
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", netin); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		inPorts = strings.Split(iip, ",")
		if len(inPorts) == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: no input ports names given in IIP, format is [port],[port],[port]...")
			os.Exit(1)
		}
	}
	fmt.Fprintln(os.Stderr, "got input ports", inPorts)

	// prepare variables
	var frame *flowd.Frame
	var err error
	var frameBuffer []*flowd.Frame

	// for each specified input port...
forPorts:
	for portIndex, inPort := range inPorts {
		//TODO if debug fmt.Fprintf(os.Stderr, "inPort=%s\n", inPort)
		// catch up any buffered IPs for new input port, in order
		for bufferIndex, frame := range frameBuffer {
			if frame.Port == inPort {
				// check for port close notification
				if frame.Type == "control" && frame.BodyType == "PortClose" {
					// done with this input port
					fmt.Fprintln(os.Stderr, "done with "+inPort)
					// remove it from buffer
					// source: https://github.com/golang/go/wiki/SliceTricks
					copy(frameBuffer[bufferIndex:], frameBuffer[bufferIndex+1:])
					// if last input port, then exit
					if portIndex == len(inPorts)-1 {
						//TODO if debug fmt.Fprintln(os.Stderr, "this was last input port")
						break forPorts
					}
					break
				}
				//TODO if debug fmt.Fprintln(os.Stderr, "sending it on")
				// send it to output port
				frame.Port = "OUT"
				if err = frame.Marshal(netout); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
				}
				// remove it from buffer
				copy(frameBuffer[bufferIndex:], frameBuffer[bufferIndex+1:])
			}
		}

		// read new frames
		for {
			// read frame
			frame, err = flowd.ParseFrame(netin)
			if err != nil {
				fmt.Fprintln(os.Stderr, err)
			}

			// if from current input port
			//TODO if debug fmt.Fprintln(os.Stderr, "got IP on input port "+frame.Port+"; inPort="+inPort)
			if frame.Port == inPort {
				// check for port close notification
				if frame.Type == "control" && frame.BodyType == "PortClose" {
					// done with this input port
					fmt.Fprintln(os.Stderr, "done with "+inPort)
					break
				}
				// send it to output port
				//TODO if debug fmt.Fprintln(os.Stderr, "sending it on")
				frame.Port = "OUT"
				if err := frame.Marshal(netout); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
				}
			} else {
				//TODO check for IP on unexpected input port (loop currentIndex+1 until end)
				// store it for later, in order
				//TODO if debug fmt.Fprintln(os.Stderr, "buffering it")
				frameBuffer = append(frameBuffer, frame)
			}
		}
	}

	// report
	fmt.Fprintln(os.Stderr, "exiting.")
}
