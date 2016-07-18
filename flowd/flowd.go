package main

import (
	"flag"
	"fmt"
	"io"
	"io/ioutil"
	"net/url"
	"os"
	"os/exec"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
	termutil "github.com/andrew-d/go-termutil"
	"github.com/oleksandr/fbp"
)

// type to hold information about an endpoint
// NOTE: duplicate from lanuch.go, because it seems unpossible to locally extend an imported struct with new methods
type endpoint struct {
	Url *url.URL
	//possibly more needed
}

type inputEndpoint endpoint
type outputEndpoint endpoint

// types to hold information on a collection of endpoints, ie. all input endpoints
// NOTE: using map of pointers, because map elements are not addressable
type inputEndpoints map[string]*inputEndpoint
type outputEndpoints map[string]*outputEndpoint

// implement flag.Value interface
//TODO these String() functions only return the first endpoint's string representation
//TODO they are also NOT used for turning an endpoint URL into a string, these functions are only used by flag package
func (e *inputEndpoints) String() string {
	for _, endpoint := range *e {
		return fmt.Sprintf("%s://%s#%s", endpoint.Url.Scheme, endpoint.Url.Host, endpoint.Url.Fragment)
	}
	return ""
}
func (e *outputEndpoints) String() string {
	for _, endpoint := range *e {
		return fmt.Sprintf("%s://%s#%s", endpoint.Url.Scheme, endpoint.Url.Host, endpoint.Url.Fragment)
	}
	return ""
}

// NOTE: can be called multiple times if there are multiple occurrences of the -in resp. -out flags
// NOTE: if only one occurrence shall be allowed, check if a required property is already set
func (e *inputEndpoints) Set(value string) error {
	if parsedUrl, err := flowd.ParseEndpointURL(value); err != nil {
		return err
	} else {
		(*e)[parsedUrl.Fragment] = &inputEndpoint{Url: parsedUrl}
	}
	return nil
}
func (e *outputEndpoints) Set(value string) error {
	if parsedUrl, err := flowd.ParseEndpointURL(value); err != nil {
		return err
	} else {
		(*e)[parsedUrl.Fragment] = &outputEndpoint{Url: parsedUrl}
	}
	return nil
}

func main() {
	// read program arguments
	inEndpoints := inputEndpoints{}
	outEndpoints := outputEndpoints{}
	var launchPath string
	var help, debug, quiet bool
	flag.Var(&inEndpoints, "in", "endpoint(s) for FBP network inports in URL format, ie. tcp://localhost:0#portname")
	flag.Var(&outEndpoints, "out", "endpoint(s) for FBP network outports in URL format, ie. tcp://localhost:0#portname")
	flag.StringVar(&launchPath, "launch", "launch", "path to the launch executable, defaults to look in PATH env")
	flag.BoolVar(&help, "h", false, "print usage information")
	flag.BoolVar(&debug, "debug", false, "give detailed event output")
	flag.BoolVar(&quiet, "quiet", false, "no informational output except errors")
	flag.Parse()
	if help {
		printUsage()
	}

	// get network definition
	var nwSource io.ReadCloser
	if termutil.Isatty(os.Stdin.Fd()) {
		// get from file
		if flag.NArg() != 1 {
			fmt.Println("ERROR: missing network definition file to run")
			printUsage()
		}

		if debug {
			fmt.Println("Reading network definition from file", flag.Arg(0))
		}
		var err error
		nwSource, err = os.Open(flag.Arg(0))
		if err != nil {
			fmt.Println(err.Error())
			os.Exit(1)
		}
	} else {
		// get from STDIN
		if debug {
			fmt.Println("Found something piped on STDIN, reading network definition from it")
		}
		nwSource = os.Stdin
	}

	// read network definition
	nwBytes, err := ioutil.ReadAll(nwSource)
	if err != nil {
		fmt.Println(err.Error())
		os.Exit(1)
	}
	if err := nwSource.Close(); err != nil {
		fmt.Println("ERROR: could not close network definition source:", err.Error())
		os.Exit(1)
	}

	// parse and validate network
	nw := &fbp.Fbp{Buffer: (string)(nwBytes)}
	if debug {
		fmt.Println("init")
	}
	nw.Init()
	if debug {
		fmt.Println("parse")
	}
	if err := nw.Parse(); err != nil {
		fmt.Println(err.Error())
		os.Exit(1)
	}
	if debug {
		fmt.Println("execute")
	}
	nw.Execute()
	if debug {
		fmt.Println("validate")
	}
	if err := nw.Validate(); err != nil {
		fmt.Println(err.Error())
		os.Exit(1)
	}
	if debug {
		fmt.Println("network definition OK")
	}

	// display all data
	if debug {
		fmt.Println("subgraph name:", nw.Subgraph)
		fmt.Println("processes:")
		for _, p := range nw.Processes {
			fmt.Println(" ", p.String(), p.Metadata)
		}
		fmt.Println("connections:")
		for _, c := range nw.Connections {
			fmt.Println(" ", c.String())
		}
		fmt.Println("input ports:")
		for name, i := range nw.Inports {
			fmt.Printf(" %s: %s\n", name, i.String())
		}
		fmt.Println("output ports:")
		for name, o := range nw.Outports {
			fmt.Printf(" %s: %s\n", name, o.String())
		}
		fmt.Println("end of parsed network info")
	}

	// network definition sanity checks
	//TODO check for multiple connections to same component's port
	//TODO decide if this should be allowed - no not usually, because then frames might be interleaved - bad if ordering is important
	//TODO

	// decide placement (know available machines, access method to them)
	//TODO

	// generate network data structures
	// prepare list of processes
	procs := make(map[string]*Process)
	for _, fbpProc := range nw.Processes {
		proc := NewProcess(fbpProc)
		if _, exists := procs[proc.Name]; exists {
			// error case
			fmt.Println("ERROR: process already exists by that name:", proc.Name)
			os.Exit(1)
		}
		procs[proc.Name] = proc
	}

	// add connections
	fmt.Println("network:")
	// add network inports
	for name, iport := range nw.Inports {
		// check if destination exists
		found := false
		for _, proc := range nw.Processes {
			if proc.Name == iport.Process {
				found = true
				break
			}
		}
		if !found {
			// destination missing
			fmt.Println("ERROR: destination process missing for inport", name)
			os.Exit(2)
		} else if debug {
			// destination found
			fmt.Printf("  inport (.fbp): %s -> %s.%s\n", name, iport.Process, iport.Port)
		}

		// prepare connection data
		//TODO maybe also get that info from FBP network metadata
		toProc := iport.Process
		toPort := iport.Port
		fromPort := name
		if _, exists := inEndpoints[fromPort]; !exists {
			// no endpoint address was given for that network inport
			fmt.Println("ERROR: no endpoint address given resp. missing -in argument for inport", name)
			os.Exit(2)
		}

		// listen input port struct
		// NOTE: resets the portname, will be added later.
		//TODO decide if internal or external port name should be used
		inEndpoints[fromPort].Url.Fragment = ""
		listenAddress := inEndpoints[fromPort].Url.String() //TODO arg->URL->string is unneccessary - actually, only launch needs to really parse it
		//TODO hack
		listenAddress = strings.Replace(listenAddress, "%40", "", 1)
		procs[toProc].InPorts = append(procs[toProc].InPorts, Port{
			LocalName:    toPort,
			LocalAddress: listenAddress, //TODO"unix://@flowd/" + toProc,
			RemoteName:   fromPort,
			//TODO currently unused
			//RemoteAddress: "unix://@flowd/" + fromProc,
		})

		// destination info
		fmt.Printf("  inport: %s at %s -> %s.%s\n", name, listenAddress, iport.Process, iport.Port)
	}
	// add regular internal connections
	for _, fbpConn := range nw.Connections {
		// check source
		if fbpConn.Source != nil { // regular connection
			//TODO implement
		} else if fbpConn.Data != "" { // source is IIP
			//TODO implement
			fmt.Printf("ERROR: connection with IIP %s to %s: currently unimplemented\n", fbpConn.Data, fbpConn.Target)
			os.Exit(2)
		}
		// check target
		if fbpConn.Target != nil {
			// regular connection
			//TODO implement
		} else {
			// check for outport, otherwise error case (unknown situation)
			//TODO implement
		}

		if debug {
			fmt.Printf("  connection (.fbp): source=%s, target=%s, data=%s\n", fbpConn.Source, fbpConn.Target, fbpConn.Data)
		}

		// prepare connection data
		fromPort := GeneratePortName(fbpConn.Source)
		toPort := GeneratePortName(fbpConn.Target)

		fromProc := fbpConn.Source.Process
		toProc := fbpConn.Target.Process

		// connecting output port struct
		remoteAddress := "unix://@flowd/" + toProc
		procs[fromProc].OutPorts = append(procs[fromProc].OutPorts, Port{
			LocalName: fromPort,
			//TODO currently unused
			//LocalAddress: "unix://@flowd/" + fromProc,
			RemoteName:    toPort,
			RemoteAddress: remoteAddress,
		})

		// listen input port struct
		localAddress := "unix://@flowd/" + toProc
		procs[toProc].InPorts = append(procs[toProc].InPorts, Port{
			LocalName:    toPort,
			LocalAddress: localAddress,
			RemoteName:   fromPort,
			//TODO currently unused
			//RemoteAddress: "unix://@flowd/" + fromProc,
		})

		fmt.Printf("  connection: %s.%s -> %s.%s at %s\n", fromProc, fromPort, toProc, toPort, remoteAddress)
	}
	// add network outports
	for name, oport := range nw.Outports {
		// check if source exists
		found := false
		for _, proc := range nw.Processes {
			if proc.Name == oport.Process {
				found = true
				break
			}
		}
		if !found {
			// source missing
			fmt.Println("ERROR: source process missing for outport", name)
			os.Exit(2)
		} else if debug {
			// source exists
			fmt.Printf("  outport (.fbp): %s.%s -> %s\n", oport.Process, oport.Port, name)
		}

		// prepare connection data
		//TODO maybe also get that info from FBP network metadata
		fromProc := oport.Process
		fromPort := oport.Port
		toPort := name
		if _, exists := outEndpoints[toPort]; !exists {
			// no endpoint address was given for that network outport
			fmt.Println("ERROR: no endpoint address given resp. missing -out argument for outport", name)
			os.Exit(2)
		}

		// connecting output port struct
		// NOTE: resets the portname, will be added later.
		//TODO decide if internal or external port name should be used
		outEndpoints[toPort].Url.Fragment = ""
		remoteAddress := outEndpoints[toPort].Url.String() //TODO arg->URL->string is unneccessary - actually, only launch needs to really parse it
		//TODO hack
		remoteAddress = strings.Replace(remoteAddress, "%40", "", 1)
		procs[fromProc].OutPorts = append(procs[fromProc].OutPorts, Port{
			LocalName: fromPort,
			//TODO currently unused
			//LocalAddress: "unix://@flowd/" + fromProc,
			RemoteName:    toPort,
			RemoteAddress: remoteAddress,
		})

		// destination info
		fmt.Printf("  outport: %s.%s -> %s to %s\n", oport.Process, oport.Port, name, remoteAddress)
	}

	// subscribe to ctrl+c to do graceful shutdown
	//TODO

	// launch network using endpoint information generated before
	launch := make(map[string]*LaunchInstance)
	exitChan := make(chan string)
	for _, proc := range nw.Processes {
		fmt.Printf("launching %s (component: %s)\n", proc.Name, proc.Component)

		// start component as subprocess, with arguments
		launch[proc.Name] = NewLaunchInstance()
		go func(name string) {
			//TODO implement exit channel behavior to goroutine ("we are going down for shutdown!")
			proc := procs[name]

			// generate arguments for launch
			var args []string
			// input endpoints
			for _, ip := range proc.InPorts {
				endpointUrl := fmt.Sprintf("%s#%s", ip.LocalAddress, ip.LocalName)
				args = append(args, "-in", endpointUrl)
			}
			// output endpoints
			for _, op := range proc.OutPorts {
				// NOTE: launch and component need to know local name of its output endpoint, not the external name
				endpointUrl := fmt.Sprintf("%s#%s", op.RemoteAddress, op.LocalName)
				args = append(args, "-out", endpointUrl)
			}
			// component pathname
			args = append(args, procs[name].Path)
			// component arguments
			args = append(args, procs[name].Arguments...)

			// TODO display launch stdout
			// start launch subprocess
			if debug {
				fmt.Println("DEBUG: arguments for launch:", launchPath, args)
			}
			cmd := exec.Command(launchPath, args...)
			/*
				cout, err := cmd.StdoutPipe()
				if err != nil {
					fmt.Println("ERROR: could not allocate pipe from component stdout:", err)
				}
				cin, err := cmd.StdinPipe()
				if err != nil {
					fmt.Println("ERROR: could not allocate pipe to component stdin:", err)
				}
				cmd.Stderr = os.Stderr
			*/
			cmd.Stdout = os.Stdout
			cmd.Stderr = os.Stderr
			if err := cmd.Start(); err != nil {
				fmt.Println("ERROR:", err)
				exitChan <- name
			}
			//TODO cin, cout copy line-by-line
			//TODO detect subprocess exit proper -> send info upstream
			cmd.Wait()
			exitChan <- name

			//defer cout.Close()
			//defer cin.Close()
		}(proc.Name)
	}

	// run while there are still components running
	//TODO is this practically useful behavior?
	for len(launch) > 0 {
		procName := <-exitChan
		//TODO detect if component exited intentionally (all data processed) or if it failed -> INFO, WARNING or ERROR and different behavior
		fmt.Println("WARNING: Component", procName, "has exited.")
		delete(launch, procName)
	}
	fmt.Println("INFO: All components have exited. Exiting.")

	// detect voluntary network shutdown
	//TODO how to decide that it should happen? should 1 component be able to trigger network shutdown?
	//TODO
}

// NOTE: enums in Go @ https://stackoverflow.com/questions/14426366/what-is-an-idiomatic-way-of-representing-enums-in-go
type Placement int

const (
	Local Placement = iota
	Remote
)

type Process struct {
	Path         string
	Arguments    []string
	Placement    Placement
	Architecture string //x86, x86_64, armv7l, armv8
	Name         string
	InPorts      []Port
	OutPorts     []Port
}

type Port struct {
	LocalName    string
	LocalAddress string

	RemoteName    string
	RemoteAddress string

	IIP string
}

func NewProcess(proc *fbp.Process) *Process {
	// take over arguments in FBP metadata under key "args"
	args := []string{}
	if _, hasArgs := proc.Metadata["args"]; hasArgs {
		args = strings.Split(proc.Metadata["args"], " ")
	}
	// return new Process struct
	return &Process{Path: proc.Component, Name: proc.Name, InPorts: []Port{}, OutPorts: []Port{}, Arguments: args}
}

func GeneratePortName(endpoint *fbp.Endpoint) string {
	if endpoint.Index == nil {
		return endpoint.Port
	} else {
		return fmt.Sprintf("%s[%d]", endpoint.Port, *endpoint.Index)
	}
}

func printUsage() {
	fmt.Println("Usage:", os.Args[0], "-in [inport-endpoint(s)]", "-out [outport-endpoint(s)]", "[network-def-file]")
	flag.PrintDefaults()
	os.Exit(1)
}

type LaunchInstance struct {
	ExitTo chan bool // command launch instance to shut down
}

func NewLaunchInstance() *LaunchInstance {
	return &LaunchInstance{ExitTo: make(chan bool)}
}
