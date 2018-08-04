package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"syscall"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
	"github.com/kballard/go-shellquote"
	"github.com/oleksandr/fbp"
)

const (
//connCapacity = 100 // 0 = synchronous
)

var (
	debug bool
	quiet bool
)

func main() {
	// profiling block
	/*
		f, err := os.Create("flowd.prof")
		if err != nil {
			panic(err)
		}
		pprof.StartCPUProfile(f)
		defer pprof.StopCPUProfile()
	*/

	// read program arguments
	var help, graph, dependencies, printruntime bool
	var olc string
	unixfbp.DefFlags()
	flag.BoolVar(&help, "h", false, "print usage information")
	//flag.BoolVar(&debug, "debug", false, "give detailed event output")
	//flag.BoolVar(&quiet, "quiet", false, "no informational output except errors")
	flag.StringVar(&olc, "olc", "", "host:port for online configuration using JSON FBP protocol")
	flag.BoolVar(&graph, "graph", false, "output visualization of given network in GraphViz format and exit")
	flag.BoolVar(&dependencies, "deps", false, "output required components for given network and exit")
	flag.BoolVar(&printruntime, "time", false, "output net runtime of network on shutdown")
	flag.Parse()
	if help {
		printUsage()
	}

	// consistency of flags
	debug = unixfbp.Debug //TODO optimize
	quiet = unixfbp.Quiet
	if debug && quiet {
		fmt.Println("ERROR: cannot have both -debug and -quiet")
		os.Exit(1)
	}

	// get network definition
	nwBytes := getNetworkDefinition()

	// parse and validate network
	nw := parseNetworkDefinition(nwBytes)
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
	procs := networkDefinition2Processes(nw)

	// subscribe to ctrl+c to do graceful shutdown
	//TODO

	// launch network
	exitChan := make(chan string)
	// launch handler(s) for INPORT, if required
	// NOTE: not necessary, because this will be picked up in startInstance()

	// launch handler(s) for NETOUT, if required
	/*
		if len(nw.Outports) > 0 {
			//fmt.Println("WARNING: NETOUT currently unimplemented")
			netout := newComponentInstance()
			go handleNetOut(netout)
			instances["NETOUT"] = netout
		}
	*/
	// launch processes
	for _, proc := range procs {
		if !quiet {
			fmt.Printf("launching %s (component: %s)\n", proc.Name, proc.Path)
		}

		// start component as subprocess, with arguments
		procs[proc.Name].Instance = newComponentInstance() //TODO optimize function call away
		go startInstance(proc, procs, nw, exitChan)        //TODO maybe make procs, nw and exitChan global
	}

	// start up online configuration
	if olc != "" {
		startOLC(olc)
	}

	// run while there are still components running
	//TODO is this practically useful behavior?
	//for len(instances) > 0 {
	var begin time.Time
	if printruntime {
		begin = time.Now()
	}
	instanceCount := len(procs)
	for instanceCount > 0 {
		//TODO check for signal here
		procName := <-exitChan
		//TODO detect if component exited intentionally (all data processed) or if it failed -> INFO, WARNING or ERROR and different behavior
		if debug {
			fmt.Println("DEBUG: Removing process instance for", procName)
		}
		// remove instance information from the process
		//instancesLock.Lock()
		//delete(instances, procName)
		//instancesLock.Unlock()
		procs[procName].Instance = nil
		instanceCount--
	}
	if !quiet {
		fmt.Println("INFO: All processes have exited. Exiting.")
	}
	if printruntime {
		fmt.Println(time.Since(begin).String())
	}

	// detect voluntary network shutdown
	//TODO how to decide that it should happen? should 1 component be able to trigger network shutdown?
}

// startProcess starts a process instance
// NOTE: own STDIN and STDOUT is given to components by default, thus enabling terminal UIs
func startInstance(proc *Process, procs Network, nw *fbp.Fbp, exitChan chan string) {
	//TODO implement exit channel behavior to goroutine ("we are going down for shutdown!")

	// start component as subprocess, with arguments
	cmd := exec.Command(proc.Path)
	// connect to STDOUT
	cout, err := cmd.StdoutPipe()
	if err != nil {
		fmt.Println("ERROR: could not allocate pipe from component stdout:", err)
	}
	//TODO optimize: try custom buffered pipe: Faster / more directly into Frame?
	/*TODO cmd.StdoutPipe() returns a pipe which is closed on wait -> data gets lost
		-> maybe AllDelivered and AllRead unnecessary with own pipe.
		cout, coutw := io.Pipe()
		cmd.Stdout = coutw
	// connect to STDIN
	/*
		cinPipe, err := cmd.StdinPipe()
		if err != nil {
			fmt.Println("ERROR: could not allocate pipe to component stdin:", err)
		}
		cin := bufio.NewWriter(cinPipe)
	*/
	// connect to STDERR
	cerr, err := cmd.StderrPipe()
	if err != nil {
		fmt.Println("ERROR: could not allocate pipe to component stderr:", err)
		exitChan <- proc.Name
	}
	// set arguments
	//TODO optimize appends and allocations
	cmd.Args = []string{proc.Name}
	// add ports for IIPs
	//TODO optimize: proc.IIPs is traversed more than once
	for _, iip := range proc.IIPs {
		if iip.Port != "ARGS" {
			// regular IIP - make port for that and create named pipe and deliver IIP
			proc.InPorts = append(proc.InPorts, Port{
				LocalPort: iip.Port,
				// leave RemotePort and RemotePort unset
			})
		}
	}
	/// add arguments for libunixfbp
	var path string
	for _, inport := range proc.InPorts {
		path = ""
		// check if this port is target of a network INPORT
		if len(nw.Inports) > 0 {
			for inPortName, inPort := range nw.Inports {
				if inPort.Process == proc.Name && inPort.Port == inport.LocalPort {
					// this component is target of a network INPORT
					///TODO see what makes more sense -- the unixfbp parameters would be consistent and are given automatically by flowd (could make exception for subnets), but nwName and using that as prefix is simpler and less parsing
					//path = fmt.Sprintf("/dev/shm/%s.%s", nwName, inPortName)
					path = unixfbp.InPorts[inPortName].Path
					if debug {
						fmt.Println("yes, INPORT-connected: INPORT", inPortName, "goes into component", proc.Name, "port", inport.LocalPort)
					}
					break
				}
			}
		}
		if path == "" {
			// make that named pipe (FIFO)
			path = fmt.Sprintf("/dev/shm/%s.%s", proc.Name, inport.LocalPort)
			os.Remove(path)
			syscall.Mkfifo(path, syscall.S_IFIFO|syscall.S_IRWXU|syscall.S_IRWXG)
		}
		// append to arguments
		cmd.Args = append(cmd.Args, "-inport", inport.LocalPort, "-inpath", path) //TODO optimize string concatenation
	}
	for _, outport := range proc.OutPorts {
		path = ""
		// check if this port is source of a network OUTPORT
		if len(nw.Outports) > 0 {
			for outPortName, outPort := range nw.Outports {
				if outPort.Process == proc.Name && outPort.Port == outport.LocalPort {
					// this component is source of a network OUTPORT
					//path = fmt.Sprintf("/dev/shm/%s.%s", nwName, outPortName)
					path = unixfbp.OutPorts[outPortName].Path
					if debug {
						fmt.Println("yes, OUTPORT-connected: component", proc.Name, "port", outport.LocalPort, "goes into OUTPORT", outPortName)
					}
					break
				}
			}
		}
		if path == "" {
			// make that named pipe (FIFO)
			path = fmt.Sprintf("/dev/shm/%s.%s", outport.RemoteProc, outport.RemotePort)
			// NOTE: create it only once - otherwise both ends would create their own version, creating weird timing-based hangs
			/*
				os.Remove(path)
				syscall.Mkfifo(path, syscall.S_IFIFO|syscall.S_IRWXU|syscall.S_IRWXG)
			*/
		}
		// append to arguments
		cmd.Args = append(cmd.Args, "-outport="+outport.LocalPort, "-outpath="+path) //TODO optimize string concatenation
	}
	// send IIPs: ARGS go into component argv, others need named pipes
	if len(proc.IIPs) > 0 && proc.IIPs[0].Port == "ARGS" {
		// add free arguments
		args, err := shellquote.Split(proc.IIPs[0].Data)
		if err != nil {
			fmt.Printf("ERROR: could not split arguments in IIP to ARGS for component %s: %s\n", proc.Name, err)
			exitChan <- proc.Name
		}
		cmd.Args = append(cmd.Args, args...)
	}
	if debug {
		fmt.Printf("argv for %s: %v\n", proc.Name, cmd.Args)
	}
	// set more file descriptors
	/*
		TODO check if cmd.ExtraFiles []*os.File makes sense to transfer the named pipes directly
		https://golang.org/pkg/os/exec/#Cmd
		is this available in all programming languages? advantages?
	*/
	// start subprocess
	if err = cmd.Start(); err != nil {
		fmt.Printf("ERROR: could not start %s: %v\n", proc.Name, err)
		exitChan <- proc.Name
	}

	// display component STDOUT
	go func() {
		// read each line and display with component name prepended
		scanner := bufio.NewScanner(cout)
		for scanner.Scan() {
			fmt.Printf("%s: %s\n", proc.Name, scanner.Text())
		}
		// notify main loop
		close(proc.Instance.AllOutputtedSTDOUT)
	}()

	// display component STDERR
	go func() {
		// read each line and display with component name prepended
		scanner := bufio.NewScanner(cerr)
		for scanner.Scan() {
			fmt.Printf("%s: %s\n", proc.Name, scanner.Text())
		}
		// notify main loop
		close(proc.Instance.AllOutputtedSTDERR)
	}()

	// deliver initial information packets/frames
	// NOTE: opening a named pipe will block until the other side has opened it
	// -> deliver the IIPs after the process has been started or before in Goroutines
	for _, iip := range proc.IIPs {
		if iip.Port != "ARGS" { //TODO optimize so that IIPs list does not have to traversed multiple times
			// get port path
			path := fmt.Sprintf("/dev/shm/%s.%s", proc.Name, iip.Port)
			// open named pipe = FIFO
			outPipe, err := os.OpenFile(path, os.O_WRONLY, os.ModeNamedPipe)
			if err != nil {
				fmt.Printf("ERROR: opening pipe to %s.%s at path %s for IIP delivery: %s - exiting.\n", proc.Name, iip.Port, path, err)
				os.Exit(2)
			}
			// create buffered writer
			outWriter := bufio.NewWriter(outPipe)
			// prepare frame
			iipFrame := &flowd.Frame{
				Type:     "data",
				BodyType: "IIP",
				Body:     []byte(iip.Data),
			}
			// send it to the component
			if err = iipFrame.Serialize(outWriter); err != nil {
				fmt.Printf("ERROR: serializing IIP for %s.%s: %s - exiting.\n", proc.Name, iip.Port, err)
				os.Exit(3)
			}
			// flush buffer
			if err = outWriter.Flush(); err != nil {
				fmt.Println("ERROR: flushing IIPs to process", proc.Name, ": ", err, "- Exiting.")
				os.Exit(3)
			}
			// close the named pipe
			if err = outPipe.Close(); err != nil {
				fmt.Printf("ERROR: closing pipe to %s.%s: %s - exiting.\n", proc.Name, iip.Port, err)
				os.Exit(3)
			}
			// success
			if !quiet {
				fmt.Printf("sent IIP to %s.%s\n", proc.Name, iip.Port)
			}
		}
	}
	// GC IIP information
	proc.IIPs = nil

	// wait for process to finish
	//err = cmd.Wait()
	// NOTE: cmd.Wait() would close the Stdout pipe (too early?), dropping unread frames
	//TODO optimize - is this still necessary? move channel receives from STDOUT and STDERR before cmd.Wait()
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
	// wait that all output from the sub-process has been read
	<-proc.Instance.AllOutputtedSTDOUT
	<-proc.Instance.AllOutputtedSTDERR
	// notify main thread
	exitChan <- proc.Name
}

func printUsage() {
	//TODOfmt.Println("Usage:", os.Args[0], "-in [inport-endpoint(s)]", "-out [outport-endpoint(s)]", "[network-def-file]")
	fmt.Println("Usage:", os.Args[0], "[network-def-file]")
	flag.PrintDefaults()
	os.Exit(1)
}

// ComponentInstances is the collection type for holding the ComponentInstance list
//TODO optimize: small optimization; instead of string maps -> int32 using symbol table, see https://syslog.ravelin.com/making-something-faster-56dd6b772b83
//TODO use ^ for procs map (type Network)
//type ComponentInstances map[string]*ComponentInstance

// ComponentInstance contains state about a running network process
type ComponentInstance struct {
	//TODO only keep sendable chans here, return receiving channels from newComponentInstance()
	AllOutputtedSTDOUT chan struct{} // tells main loop that all output the exited component sent to STDOUT are now read and displayed
	AllOutputtedSTDERR chan struct{} // tells main loop that all output the exited component sent to STDERR are now read and displayed
	Cmd                *exec.Cmd     // subprocess state
}

func newComponentInstance() *ComponentInstance {
	return &ComponentInstance{AllOutputtedSTDOUT: make(chan struct{}), AllOutputtedSTDERR: make(chan struct{})}
}

/*TODO
func handleSignals() {
	signalChannel := make(chan os.Signal, 1) // subscribe to notification on signal
	signal.Notify(signalChannel,
		syscall.SIGHUP,
		syscall.SIGTERM,
		syscall.SIGQUIT,
		syscall.SIGINT,
		syscall.SIGKILL,
		syscall.SIGUSR1,
	)
	for sig := range signalChannel {
		if sig == syscall.SIGHUP {
			//TODO reload network
		} else if sig == syscall.SIGUSR1 {
			//TODO reopen log file
		} else if sig == syscall.SIGTERM || sig == syscall.SIGQUIT || sig == syscall.SIGINT {
			fmt.Println("Shutdown signal caught")
			go func() {
				select {
				// exit if graceful shutdown not finished in 60 sec.
				case <-time.After(time.Second * 60):
					fmt.Println("ERROR: Graceful shutdown timed out")
					os.Exit(1)
				}
			}()
			//TODO shut all down here
			fmt.Println("Shutdown completed, exiting.")
			break
			//TODO optimize breaks/returns here
		} else {
			fmt.Println("Shutdown, unknown signal caught")
			break
		}
	}
}
*/
