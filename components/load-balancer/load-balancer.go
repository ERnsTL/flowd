package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
)

//TODO untested - add example network and test different scenarios

/*
Features:

- will reconnect the outports (handled in a separate Goroutine because the open call may block) waiting for someone to (re-)connect
- gives warning if no takers available
- can switch enabled outports by command on SWITCH port so that the worker on that pipe can gracefully shut down
- uses simple round-robin load balancing
*/

//TODO measure service uptime and print it once a day
//TODO add feedback-based balancing = know, how many frames / packets are queued on each output port, then write to the one with the shortest queue.

var netin *bufio.Reader // only for checking if buffered data is available

func main() {
	// flag variables
	var control bool
	// get configuration from flags
	unixfbp.DefFlags()
	flag.BoolVar(&control, "switch", false, "open control port to switch active outports")
	flag.Parse()
	if flag.NArg() == 0 {
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
	if control {
		// open control port
		_, _, err = unixfbp.OpenOutPort("SWITCH")
		if err != nil {
			fmt.Println("ERROR:", err)
			os.Exit(2)
		}
	}
	outHandlers := make([]chan *flowd.Frame, len(unixfbp.OutPorts))
	outPortNames := make([]string, len(unixfbp.OutPorts)) // NOTE: because cannot take address of map[string]bool entry
	portsAvailable := make([]bool, len(unixfbp.OutPorts)) //TODO optimize: keep list of ready-to-send ports in own list -> no iteration over portsAvailable
	var curIndex int
	for portName, _ := range unixfbp.OutPorts {
		// create buffered chan
		outHandlers[curIndex] = make(chan *flowd.Frame, 5)
		// handle that outport
		handleOutPort(portName, outHandlers[curIndex], &portsAvailable[curIndex])
		outPortNames[curIndex] = portName
		// next index
		curIndex++
	}
	if !unixfbp.Quiet {
		fmt.Fprintln(os.Stderr, "got output ports", unixfbp.OutPorts)
	}

	// control packet receiver
	controlChan := make(chan *flowd.Frame, 1)
	if control {
		// start handler in Goroutine
		go func() {
			var frame *flowd.Frame
			var err error
			controlR := unixfbp.InPorts["SWITCH"].Reader
			for {
				if frame, err = flowd.Deserialize(controlR); err != nil {
					fmt.Fprintln(os.Stderr, err)
				} else {
					// forward frame to main loop
					controlChan <- frame
				}
			}
		}()
	}
	// main loop
	var frame *flowd.Frame
	var enable bool
	curIndex = 0
	var tryIndex int
	for {
		// got a switch command?
		if len(controlChan) > 0 {
			// receive it
			frame = <-controlChan
			// parse command
			enablePorts := strings.Split(strings.TrimSpace(string(frame.Body)), " ")
			if !unixfbp.Quiet {
				fmt.Fprintln(os.Stderr, "got request to switch outports:", enablePorts)
			}
			// check port list
			portsOK := true
			for _, portName := range enablePorts {
				if _, found := unixfbp.OutPorts[portName]; !found {
					fmt.Fprintln(os.Stderr, "WARNING: outport unknown:", portName, "- discarding switch command.")
					portsOK = false
					break
				}
			}
			if portsOK {
				// set requested port availability
				for index, _ := range portsAvailable {
					// should that be available or not?
					enable = false
					for _, portName := range enablePorts {
						if portName == outPortNames[index] {
							// port shall be enabled
							enable = true
							break
						}
					}
					// set value
					portsAvailable[index] = enable
				}
			}
		}

		// read frame
		frame, err = flowd.Deserialize(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// check if current port is available
		tryIndex = curIndex //TODO optimize - do without that; and without looping over portsAvailable
	tryAgain:
		if !portsAvailable[curIndex] {
			// try next outport, wrapping around if necessary
			curIndex++
			if curIndex == len(portsAvailable) {
				curIndex = 0
			}
			// full loop done?
			if curIndex == tryIndex {
				fmt.Fprintln(os.Stderr, "WARNING: no takers available - discarding frame!")
				continue
			}
			// try that one
			goto tryAgain
		}

		// send it to current outport
		outHandlers[curIndex] <- frame

		// go to next outport, wrapping around if necessary
		curIndex++
		if curIndex == len(portsAvailable) {
			curIndex = 0
		}
	}
}

func handleOutPort(portName string, inChan <-chan *flowd.Frame, status *bool) {
	// connect port - this may block, which is fine
	outW, _, err := unixfbp.OpenOutPort(portName)
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	*status = true
	defer outW.Flush()
	// forward frames
	for frame := range inChan {
		// forward it
		if err = frame.Serialize(outW); err != nil {
			//TODO handle EOF gracefully
			if err == io.EOF {
				fmt.Fprintln(os.Stderr, "EOF")
			} else {
				// serious error
				fmt.Fprintln(os.Stderr, "ERROR: serializing frame:", err.Error())
			}
			// take out of list of available ports
			*status = false
			// reset and try to connect again - will block until other side connects
			outW, _, err = unixfbp.OpenOutPort(portName)
			if err != nil {
				fmt.Println("ERROR:", err)
				os.Exit(2)
			}
			*status = true
			defer outW.Flush()
		}
		// flush if no packets waiting on input
		if netin.Buffered() == 0 {
			if err = outW.Flush(); err != nil {
				fmt.Fprintf(os.Stderr, "ERROR: flushing port '%s': %s\n", portName, err)
			}
		}
	}
}
