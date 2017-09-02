package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"sync"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/miolini/datacounter"
)

const (
	connCapacity = 100 // 0 = synchronous
)

var (
	instancesLock sync.RWMutex // lock to the process instances array
)

func main() {
	// profiling block
	/*
		f, err := os.Create("flowd.prof")
		if err != nil {
			log.Fatal(err)
		}
		pprof.StartCPUProfile(f)
		defer pprof.StopCPUProfile()
	*/

	// read program arguments
	var help, debug, quiet, graph, dependencies bool
	var olc string
	flag.BoolVar(&help, "h", false, "print usage information")
	flag.BoolVar(&debug, "debug", false, "give detailed event output")
	flag.BoolVar(&quiet, "quiet", false, "no informational output except errors")
	flag.StringVar(&olc, "olc", "", "host:port for online configuration using JSON FBP protocol")
	flag.BoolVar(&graph, "graph", false, "output visualization of given network in GraphViz format and exit")
	flag.BoolVar(&dependencies, "deps", false, "output required components for given network and exit")
	flag.Parse()
	if help {
		printUsage()
	}

	// get network definition
	nwBytes := getNetworkDefinition(debug)

	// parse and validate network
	nw := parseNetworkDefinition(nwBytes, debug)
	if olc != "" && (len(nw.Inports) > 0 || len(nw.Outports) > 0) {
		fmt.Println("ERROR: NETIN and NETOUT require -olc, otherwise use TCP/UDP/SSH/UNIX/etc. components")
		os.Exit(1)
	}

	// display all data
	if debug {
		displayNetworkDefinition(nw)
	}

	// network definition sanity checks
	//TODO check for multiple connections to same component's port
	//TODO decide if this should be allowed - no not usually, because then frames might be interleaved - bad if ordering is important

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

	// output required components for this network
	if dependencies {
		dependencies := map[string]bool{} // use map to ignore duplicates (uniq)
		for _, proc := range nw.Processes {
			dependencies[proc.Component] = true
			for key, value := range proc.Metadata {
				//TODO not full solution: cannot give multiple dep= keys (last one counts); need to put that into single deps= key using separator
				//TODO not full solution: parser does not allow common characters in file names: .
				if key == "dep" {
					dependencies[value] = true
				}
			}
		}
		for component := range dependencies {
			fmt.Println(component)
		}
		return
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
		go func(proc *Process) { //TODO move into own function
			//TODO implement exit channel behavior to goroutine ("we are going down for shutdown!")

			// start component as subprocess, with arguments
			cmd := exec.Command(proc.Path)
			// connect to STDOUT
			cout, err := cmd.StdoutPipe()
			if err != nil {
				fmt.Println("ERROR: could not allocate pipe from component stdout:", err)
			}
			/*TODO use something more direct than Stdoutpipe kernel object with buffer -> bufio.Reader in handleComponentOutput()
			-> do not have to flush after every frame
			TODO maybe useful: buffered pipe @ https://github.com/kr/spdy/blob/master/spdyframing/pipe.go
			tried Go-level pipe (short writes after a while):
			cout, coutw := io.Pipe()
			cmd.Stdout = coutw
			tried using buffer (does not wake up, nothing gets sent):
			cout := new(bytes.Buffer)
			cmd.Stdout = cout
			tried custom pipe (short writes after a while):
			cout := &pipe{condition: sync.NewCond(new(sync.Mutex))}
			cmd.Stdout = cout
			-> short writes still come up using syscall OS-level pipe.
			*/
			// connect to STDIN
			cinPipe, err := cmd.StdinPipe()
			if err != nil {
				fmt.Println("ERROR: could not allocate pipe to component stdin:", err)
			}
			cin := bufio.NewWriter(cinPipe)
			// connect to STDERR
			cerr, err := cmd.StderrPipe()
			if err != nil {
				fmt.Println("ERROR: could not allocate pipe to component stderr:", err)
			}
			// start subprocess
			if err = cmd.Start(); err != nil {
				fmt.Printf("ERROR: could not start %s: %v\n", proc.Name, err)
				exitChan <- proc.Name
			}

			// display component stderr
			go func() {
				// prepare instance AllOutputted chan
				donechan := instances[proc.Name].AllOutputted
				// read each line and display with component name prepended
				scanner := bufio.NewScanner(cerr)
				for scanner.Scan() {
					fmt.Printf("%s: %s\n", proc.Name, scanner.Text())
				}
				// notify main loop
				close(donechan)
			}()

			// first deliver initial information packets/frames
			for i := 0; i < len(proc.IIPs); i++ {
				iipInfo := proc.IIPs[i] //TODO optimize reduce allocations here
				port := iipInfo.Port
				data := iipInfo.Data
				iip := &flowd.Frame{
					Type:     "data",
					BodyType: "IIP", //TODO maybe this could be user-defined, but would make argument-passing more complicated for little return
					Port:     port,
					//ContentType: "text/plain", // is a string from commandline, unlikely to be binary = application/octet-stream, no charset info needed since on same platform
					Extensions: nil,
					Body:       []byte(data),
				}
				if !quiet {
					fmt.Printf("in xfer 1 IIP to %s.%s\n", proc.Name, port)
				}
				if err = iip.Marshal(cin); err != nil {
					fmt.Println("ERROR sending IIP to port", port, ": ", err, "- Exiting.")
					os.Exit(3)
				}
			}
			// flush buffer
			if err = cin.Flush(); err != nil {
				fmt.Println("ERROR flushing IIPs to process", proc.Name, ": ", err, "- Exiting.")
				os.Exit(3)
			}
			// GC it
			proc.IIPs = nil

			// start handler for regular packets/frames
			//instancesLock.RLock()
			inputChan := instances[proc.Name].Input
			//instancesLock.RUnlock()
			go handleComponentInput(inputChan, proc, cin, debug, quiet) // TODO maybe make debug and quiet global

			// NOTE: this using manual buffering
			go handleComponentOutput(proc, instances, cout, debug, quiet)

			// wait for process to finish
			//err = cmd.Wait()
			// NOTE: cmd.Wait() would close the Stdout pipe (too early?), dropping unread frames
			state, err := cmd.Process.Wait()
			cmd.ProcessState = state
			if err != nil {
				fmt.Printf("ERROR waiting for exit of component %s: %v\n", proc.Name, err)
			}
			// check exit status
			if !cmd.ProcessState.Success() {
				//TODO warning or error?
				fmt.Println("ERROR: Processs", proc.Name, "exited unsuccessfully.")
				//TODO how to react properly? shut down network?
			} else if !quiet {
				fmt.Println("INFO: Process", proc.Name, "exited normally.")
			}
			// notify main thread
			exitChan <- proc.Name
		}(proc)
	}

	// start up online configuration
	if olc != "" {
		startOLC(olc)
	}

	// run while there are still components running
	//TODO is this practically useful behavior?
	//for len(instances) > 0 {
	instanceCount := len(instances)
	for instanceCount > 0 {
		procName := <-exitChan
		//TODO detect if component exited intentionally (all data processed) or if it failed -> INFO, WARNING or ERROR and different behavior
		if debug {
			fmt.Println("DEBUG: Removing process instance for", procName)
		}
		// wait that all frames sent by the exited component have been delivered
		// NOTE: otherwise the port close notification would be injected,
		// because cmd.Wait() and exitChan easily overtakes handleComponentOutput() goroutine
		<-instances[procName].AllDelivered
		// same for exited component's stderr output
		<-instances[procName].AllOutputted
		// send PortClose notifications to all affected downstream components
		proc := procs[procName]
		var found bool
	nextRemoteInstance:
		for _, port := range proc.OutPorts {
			// check for other processes still connected to that remote port
			found = false
			// for all running instances...
			//TODO optimize - dont go through whole list -> needs better network datastructure
			for procNameLookup, instance := range instances {
				// only not-deleted instances are considered alive
				if instance.Deleted {
					continue
				}
				// get process = static network info
				procLookup := procs[procNameLookup]
				// check for outport to same process on same remote port
				for _, outPort := range procLookup.OutPorts {
					if outPort.RemoteProc == port.RemoteProc && outPort.RemotePort == port.RemotePort {
						// still got a running process feeding into that remote port
						// NOTE: one match will be same connection as one currently notifying remote
						if !found {
							found = true
						} else {
							// found second match
							continue nextRemoteInstance
						}
					}
				}
			}
			// create notification for the remote port
			notification := flowd.PortClose(port.RemotePort)
			// send it to the remote process
			//instancesLock.RLock()
			//instances[port.RemoteProc].Input <- SourceFrame{Frame: &notification, Source: proc}
			input := instances[port.RemoteProc].Input
			//instancesLock.RUnlock()
			input <- SourceFrame{Frame: &notification, Source: proc}
			/*
				TODO
				if instance, found := instances[port.RemoteProc]; found {
					instance.Input <- SourceFrame{Frame: &notification, Source: proc}
				} else {
					fmt.Printf("ERROR: main loop: Instance %s not found / deleted - exiting.\n", port.RemoteProc)
					os.Exit(1)
				}
			*/
			//instancesLock.RUnlock()
		}
		// remove from list of instances
		//instancesLock.Lock()
		//delete(instances, procName)
		//instancesLock.Unlock()
		instances[procName].Deleted = true
		instanceCount--
	}
	if !quiet {
		fmt.Println("INFO: All processes have exited. Exiting.")
	}

	// detect voluntary network shutdown
	//TODO how to decide that it should happen? should 1 component be able to trigger network shutdown?
}

func handleComponentInput(input <-chan SourceFrame, proc *Process, cin *bufio.Writer, debug bool, quiet bool) {
	// use write counter only if !quiet or even debug
	///TODO re-enable counting functionality
	/*
		var countw *datacounter.WriterCounter
		var cinw io.Writer
		if !quiet {
			countw = datacounter.NewWriterCounter(cin)
			cinw = countw
		} else {
			cinw = cin
		}
	*/
	// other initializations
	//var oldCount uint64
	var frame SourceFrame
	for {
		// wait for frame
		frame = <-input

		// check for EOF = channel got closed -> exit goroutine
		if frame.Source == nil {
			fmt.Println("EOF from network input channel - closing.")
			//TODO also close cin here?
			break
		}

		// received fine
		if debug {
			fmt.Println("received frame type", frame.Type, "and data type", frame.BodyType, "for port", frame.Port, "with headers", frame.Extensions, "and body:", (string)(frame.Body)) //TODO difference between this and string(fr.Body) ?
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
		if err := frame.Marshal(cin); err != nil {
			fmt.Println("net in: WARNING: could not marshal received frame into component STDIN - discarding.")
		}
		//TODO optimize: Flush is very expensive; but flushing only every nth frame causes hang because of undelivered frames
		if err := cin.Flush(); err != nil {
			fmt.Println("net in: WARNING: could not flush frame into component STDIN - ignoring.")
		}
		// status message
		/*///TODO re-enable that functionality
		if debug {
			fmt.Println("STDIN wrote", countw.Count()-oldCount, "bytes from", frame.Source.Name, "to component stdin of", proc.Name)
			oldCount = countw.Count()
		} else if !quiet {
			fmt.Println("in xfer", countw.Count()-oldCount, "bytes on", frame.Port, "from", frame.Source.Name)
			oldCount = countw.Count()
		}
		*/
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
				// normal component shutdown
				if debug {
					fmt.Println("EOF from component stdout - exiting.")
				}
			} else {
				fmt.Printf("ERROR parsing frame from stdout of component %s: %s - exiting.\n", proc.Name, err)
			}
			// notify that all messages from component were delivered - main loop waits for this
			//instancesLock.RLock()
			//close(instances[proc.Name].AllDelivered)
			instance := instances[proc.Name]
			//instancesLock.RUnlock()
			close(instance.AllDelivered)
			return
		}

		if debug {
			fmt.Println("STDOUT received frame type", frame.Type, "and data type", frame.BodyType, "for port", frame.Port, "with headers", frame.Extensions, "and body:", string(frame.Body))
		}

		// check for valid outport
		//TODO convert from array to map
		var outPort *Port
		for _, port := range proc.OutPorts {
			if port.LocalPort == frame.Port {
				outPort = &port
				break
			}
		}
		if outPort == nil {
			fmt.Printf("net out: ERROR: component %s tried sending to undeclared port %s. Exiting.\n", proc.Name, frame.Port)
			return
		}

		// rewrite frame.Port to match the other side's input port name
		// NOTE: This makes multiple input ports possible
		frame.Port = outPort.RemotePort

		// send to input channel of target process
		//instancesLock.RLock()
		//instances[outPort.RemoteProc].Input <- SourceFrame{Source: proc, Frame: frame}
		input := instances[outPort.RemoteProc].Input
		//instancesLock.RUnlock()
		if debug {
			fmt.Printf("net out: send from %s to %s: channel has %d free\n", proc.Name, outPort.RemoteProc, cap(input)-len(input))
		}
		input <- SourceFrame{Source: proc, Frame: frame}
		/*
			TODO
			if instance, found := instances[outPort.RemoteProc]; found {
				instance.Input <- SourceFrame{Source: proc, Frame: frame}
			} else {
				fmt.Printf("ERROR: handleComponentOutput(): Instance %s not found / deleted - exiting.\n", outPort.RemoteProc)
				os.Exit(2)
			}
		*/
		//instancesLock.RUnlock()

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
//TODO optimize: small optimization; instead of string maps -> int32 using symbol table, see https://syslog.ravelin.com/making-something-faster-56dd6b772b83
type ComponentInstances map[string]*ComponentInstance

// ComponentInstance contains state about a running network process
type ComponentInstance struct {
	//TODO only keep sendable chans here, return receiving channels from newComponentInstance()
	ExitTo       chan bool        // tell command handler - and therefore component - to shut down	//TODO change to struct{} and close it
	Input        chan SourceFrame // handler inbox for sending a frame to it
	AllDelivered chan struct{}    // tells mail loop that all messages the exited component sent to STDOUT are now delivered
	AllOutputted chan struct{}    // tells main loop that all output the exited component sent to STDERR are now read and displayed
	Deleted      bool             //TODO mark entry as deleted to avoid concurrent read (many) and write (one for each instance on cleanup) panic on instance map
}

func newComponentInstance() *ComponentInstance {
	return &ComponentInstance{ExitTo: make(chan bool), Input: make(chan SourceFrame, connCapacity), AllDelivered: make(chan struct{}), AllOutputted: make(chan struct{})}
}

// SourceFrame contains an actual frame plus sender information used in handler functions
type SourceFrame struct {
	*flowd.Frame
	Source *Process
}
