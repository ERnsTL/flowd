package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"

	flowd "github.com/ERnsTL/flowd/libflowd"
	"github.com/miolini/datacounter"
)

func main() {
	// read program arguments
	var help, debug, quiet, graph bool
	flag.BoolVar(&help, "h", false, "print usage information")
	flag.BoolVar(&debug, "debug", false, "give detailed event output")
	flag.BoolVar(&quiet, "quiet", false, "no informational output except errors")
	flag.BoolVar(&graph, "graph", false, "output visualization of given network in GraphViz format and exit")
	flag.Parse()
	if help {
		printUsage()
	}

	// get network definition
	nwBytes := getNetworkDefinition(debug)

	// parse and validate network
	nw := parseNetworkDefinition(nwBytes, debug)

	// display all data
	if debug {
		displayNetworkDefinition(nw)
	}

	// network definition sanity checks
	//TODO check for multiple connections to same component's port
	//TODO decide if this should be allowed - no not usually, because then frames might be interleaved - bad if ordering is important
	//TODO

	// output graph visualization
	// NOTE: originally intended to output the parsed graph (fbp.Fbp type), but that does not have Inports and Outports process names nicely available and IIP special cases
	if graph {
		if err := exportNetworkGraph(nw); err != nil {
			fmt.Println("ERROR: generating graph visualization: ", err)
			os.Exit(1)
		} else {
			return
		}
	}

	// generate network data structures
	procs := networkDefinition2Processes(nw, debug)

	// subscribe to ctrl+c to do graceful shutdown
	//TODO

	// launch network
	instances := make(ComponentInstances)
	exitChan := make(chan string)
	// launch handler for NETOUT, if required
	if len(nw.Outports) > 0 {
		netout := newComponentInstance()
		go handleNetOut(netout)
		instances["NETOUT"] = netout
	}
	// launch processes
	for _, proc := range procs {
		if !quiet {
			fmt.Printf("launching %s (component: %s)\n", proc.Name, proc.Path)
		}

		// start component as subprocess, with arguments
		instances[proc.Name] = newComponentInstance()
		go func(name string) { //TODO move into own function
			//TODO implement exit channel behavior to goroutine ("we are going down for shutdown!")
			proc := procs[name]

			// start component as subprocess, with arguments
			cmd := exec.Command(proc.Path)
			cout, err := cmd.StdoutPipe()
			if err != nil {
				fmt.Println("ERROR: could not allocate pipe from component stdout:", err)
			}
			cin, err := cmd.StdinPipe()
			if err != nil {
				fmt.Println("ERROR: could not allocate pipe to component stdin:", err)
			}
			//cmd.Stderr = os.Stderr
			cerr, err := cmd.StderrPipe()
			if err != nil {
				fmt.Println("ERROR: could not allocate pipe to launch stdin:", err)
			}
			if err := cmd.Start(); err != nil {
				fmt.Println("ERROR:", err)
				exitChan <- name
			}
			defer cout.Close()
			defer cin.Close()

			// display component stderr
			go func() {
				scanner := bufio.NewScanner(cerr)
				for scanner.Scan() {
					fmt.Printf("%s: %s\n", proc.Name, scanner.Text())
				}
			}()

			// first deliver initial information packets/frames
			if len(proc.IIPs) > 0 {
				for port, data := range proc.IIPs {
					iip := &flowd.Frame{
						Type:     "data",
						BodyType: "IIP", //TODO maybe this could be user-defined, but would make argument-passing more complicated for little return
						Port:     port,
						//ContentType: "text/plain", // is a string from commandline, unlikely to be binary = application/octet-stream, no charset info needed since on same platform
						Extensions: nil,
						Body:       []byte(data),
					}
					if !quiet {
						fmt.Println("in xfer 1 IIP to", port)
					}
					if err := iip.Marshal(cin); err != nil {
						fmt.Println("ERROR sending IIP to port", port, ": ", err, "- Exiting.")
						os.Exit(3)
					}
				}
				// GC it
				proc.IIPs = nil
			}

			// start handler for regular packets/frames
			go handleComponentInput(instances[proc.Name].Input, proc, cin, debug, quiet) // TODO maybe make debug and quiet global

			// NOTE: this using manual buffering
			go handleComponentOutput(proc, instances, cout, debug, quiet)

			//TODO detect subprocess exit proper -> send info upstream
			cmd.Wait()
			exitChan <- name

			//TODO
			//defer lout.Close()
			//defer lerr.Close()
		}(proc.Name)
	}

	// run while there are still components running
	//TODO is this practically useful behavior?
	for len(instances) > 0 {
		procName := <-exitChan
		//TODO detect if component exited intentionally (all data processed) or if it failed -> INFO, WARNING or ERROR and different behavior
		fmt.Println("WARNING: Process", procName, "has exited.")
		delete(instances, procName)
	}
	fmt.Println("INFO: All processes have exited. Exiting.")

	// detect voluntary network shutdown
	//TODO how to decide that it should happen? should 1 component be able to trigger network shutdown?
}

func handleComponentInput(input <-chan SourceFrame, proc *Process, cin io.WriteCloser, debug bool, quiet bool) {
	countw := datacounter.NewWriterCounter(cin)
	var oldCount uint64
	var frame SourceFrame
	for {
		// wait for frame
		frame = <-input

		// check for EOF = channel got closed -> exit goroutine
		if frame.Source == nil {
			fmt.Println("EOF from network input channel - closing.")
			break
		}

		// received fine
		if debug {
			fmt.Println("received frame type", frame.Type, "and data type", frame.BodyType, "for port", frame.Port, "with body:", (string)(frame.Body)) //TODO difference between this and string(fr.Body) ?
		}

		// check frame Port header field if it matches the name of this input endpoint
		//TODO convert from array to map
		found := false
		for _, port := range proc.InPorts {
			if port.LocalPort == frame.Port {
				found = true
				break
			}
		}
		if !found {
			fmt.Println("net in: WARNING: frame for wrong/undeclared inport", frame.Port, " - discarding.")
			// discard frame
			continue
		}
		// forward frame to component
		if err := frame.Marshal(countw); err != nil {
			fmt.Println("net in: WARNING: could not marshal received frame into component STDIN - discarding.")
		}

		// status message
		if debug {
			fmt.Println("STDIN wrote", countw.Count()-oldCount, "bytes from", frame.Source.Name, "to component stdin")
		} else if !quiet {
			fmt.Println("in xfer", countw.Count()-oldCount, "bytes on", frame.Port, "from", frame.Source.Name)
		}
		oldCount = countw.Count()
	}
}

func handleComponentOutput(proc *Process, instances ComponentInstances, cout io.ReadCloser, debug bool, quiet bool) {
	defer cout.Close()
	countr := datacounter.NewReaderCounter(cout)
	bufr := bufio.NewReader(countr)
	var oldCount uint64
	var frame *flowd.Frame
	var err error
	for {
		frame, err = flowd.ParseFrame(bufr)

		// check for error
		if err != nil {
			if err == io.EOF {
				fmt.Println("EOF from component stdout. Exiting.")
			} else {
				fmt.Println("ERROR parsing frame from component stdout:", err, "- Exiting.")
			}
			return
		}

		if debug {
			fmt.Println("STDOUT received frame type", frame.Type, "and data type", frame.BodyType, "for port", frame.Port, "with contents:", string(frame.Body))
		}

		// write out to network
		//TODO convert from array to map
		var outPort *Port
		for _, port := range proc.OutPorts {
			if port.LocalPort == frame.Port {
				outPort = &port
				break
			}
		}
		// check for valid outport
		if outPort == nil {
			fmt.Printf("net out: ERROR: component tried sending to undeclared port %s. Exiting.\n", outPort.LocalPort)
			return
		}

		// rewrite frame.Port to match the other side's input port name
		// NOTE: This makes multiple input ports possible
		frame.Port = outPort.RemotePort

		// send to input channel of target process
		instances[outPort.RemoteProc].Input <- SourceFrame{Source: proc, Frame: frame}

		// status message
		if debug {
			// NOTE: if accurate byte count was desired, then add -uint64(len(outPort.LocalPort))+uint64(len(outPort.RemotePort))
			fmt.Println("net out wrote", countr.Count()-oldCount, "bytes to port", outPort.LocalPort, "=", outPort.RemoteProc+"."+outPort.RemotePort, "with body:", string(frame.Body))
		} else if !quiet {
			fmt.Println("out xfer", countr.Count()-oldCount, "bytes to", outPort.LocalPort, "=", outPort.RemoteProc+"."+outPort.RemotePort)
		}
		oldCount = countr.Count()
	}
}

func handleNetOut(instance *ComponentInstance) {
	var frame SourceFrame
	for {
		frame = <-instance.Input

		// check for EOF
		if frame.Source == nil {
			fmt.Println("NETOUT received EOF on channel, exiting.")
			break
		}

		//TODO implement FBP websocket protocol and send to that
		fmt.Printf("NETOUT received frame from %s for port %s: %s - Discarding.\n", frame.Source.Name, frame.Port, string(frame.Body))
	}
}

func printUsage() {
	//TODOfmt.Println("Usage:", os.Args[0], "-in [inport-endpoint(s)]", "-out [outport-endpoint(s)]", "[network-def-file]")
	fmt.Println("Usage:", os.Args[0], "[network-def-file]")
	flag.PrintDefaults()
	os.Exit(1)
}

// ComponentInstances is the collection type for holding the ComponentInstance list
type ComponentInstances map[string]*ComponentInstance

// ComponentInstance contains state about a running network process
type ComponentInstance struct {
	//TODO only keep sendable chans here, return receiving channels from newComponentInstance()
	ExitTo chan bool        // tell command handler - and therefore component - to shut down
	Input  chan SourceFrame // handler inbox for sending a frame to it
}

func newComponentInstance() *ComponentInstance {
	return &ComponentInstance{ExitTo: make(chan bool), Input: make(chan SourceFrame)}
}

// SourceFrame contains an actual frame plus sender information used in handler functions
type SourceFrame struct {
	*flowd.Frame
	Source *Process
}
