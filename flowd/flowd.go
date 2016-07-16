package main

import (
	"flag"
	"fmt"
	"io"
	"io/ioutil"
	"net/url"
	"os"
	"os/exec"

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
	var help, debug, quiet bool
	flag.Var(&inEndpoints, "in", "endpoint(s) for FBP network inports in URL format, ie. tcp://localhost:0#portname")
	flag.Var(&outEndpoints, "out", "endpoint(s) for FBP network outports in URL format, ie. tcp://localhost:0#portname")
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
			fmt.Println("Reading network definition from file")
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
			fmt.Println(" ", p.String())
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
	for _, fbpConn := range nw.Connections {
		// prepare connection data
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

		fmt.Printf("  connection: source=%s, target=%s, data=%s\n", fbpConn.Source, fbpConn.Target, fbpConn.Data)
		fromPort := GeneratePortName(fbpConn.Source)
		toPort := GeneratePortName(fbpConn.Target)

		fromProc := fbpConn.Source.Process
		toProc := fbpConn.Target.Process

		// connecting output port
		procs[fromProc].OutPorts = append(procs[fromProc].OutPorts, Port{
			LocalName: fromPort,
			//TODO currently unused
			//LocalAddress: "unix://@flowd/" + fromProc,
			RemoteName:    toPort,
			RemoteAddress: "unix://@flowd/" + toProc,
		})

		// listen input port
		procs[toProc].InPorts = append(procs[toProc].InPorts, Port{
			LocalName:    toPort,
			LocalAddress: "unix://@flowd/" + toProc,
			RemoteName:   fromPort,
			//TODO currently unused
			//RemoteAddress: "unix://@flowd/" + fromProc,
		})
	}
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
			fmt.Printf("  inport: %s -> %s.%s\n", name, iport.Process, iport.Port)
		}

		// add connections
		//TODO maybe also get that info from FBP network metadata
		toProc := iport.Process
		toPort := iport.Port
		fromPort := name
		if _, exists := inEndpoints[fromPort]; !exists {
			// no endpoint address was given for that network inport
			fmt.Println("ERROR: no endpoint address given resp. missing -in argument for inport", name)
			os.Exit(2)
		}
		listenAddress := inEndpoints[fromPort].Url.String() //TODO arg->URL->string is unneccessary - actually, only launch needs to really parse it
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
		if found {
			// source exists
			fmt.Printf("  outport: %s.%s -> %s\n", oport.Process, oport.Port, name)
		} else {
			// source missing
			fmt.Println("ERROR: source process missing for outport", name)
			os.Exit(2)
		}

		// add to connections
		//TODO maybe also get that info from FBP network metadata
		//TODO implement - get info from flag where that port goes to
	}

	// subscribe to ctrl+c to do graceful shutdown
	//TODO

	// launch network
	// TODO display launch stdout
	for _, proc := range nw.Processes {
		//TODO exit channel to goroutine
		//TODO exit channel from goroutine
		fmt.Printf("launching %s (component: %s)\n", proc.Name, proc.Component)

		//TODO need to have ports generated here -> generate arguments for launch

		go func() {
			// start component as subprocess, with arguments
			//TODO generate arguments, which arguments?
			cmd := exec.Command(flag.Arg(0), flag.Args()[1:]...)
			cout, err := cmd.StdoutPipe()
			if err != nil {
				fmt.Println("ERROR: could not allocate pipe from component stdout:", err)
			}
			cin, err := cmd.StdinPipe()
			if err != nil {
				fmt.Println("ERROR: could not allocate pipe to component stdin:", err)
			}
			cmd.Stderr = os.Stderr
			if err := cmd.Start(); err != nil {
				fmt.Println("ERROR:", err)
				os.Exit(2)
			}
			defer cout.Close()
			defer cin.Close()
		}()
	}

	// detect voluntary network shutdown (how to decide that it should happen?)
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
	//TODO make use of arguments in proc.Metadata
	return &Process{Path: proc.Component, Name: proc.Name, InPorts: []Port{}, OutPorts: []Port{}}
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
