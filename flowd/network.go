package main

import (
	"flag"
	"fmt"
	"io"
	"io/ioutil"
	"os"

	termutil "github.com/andrew-d/go-termutil"
	"github.com/oleksandr/fbp"
)

// The Network type holds a map of processes
type Network map[string]*Process

// Process holds information about a network process
//TODO optimize: small optimization; instead of string maps -> int32 using symbol table, see https://syslog.ravelin.com/making-something-faster-56dd6b772b83
type Process struct {
	Path     string
	Name     string
	InPorts  []Port
	OutPorts []Port
	IIPs     []IIP
}

// IIP holds information about an IIP to be delivered
type IIP struct {
	Port string
	Data string
}

// Port holds connection information about a process port (connection), whether input or output
//TODO optimize: convert network information to <E,V> = edges and vertices = nodes and connections structure
type Port struct {
	LocalPort  string
	RemotePort string
	RemoteProc string
}

func getNetworkDefinition(debug bool) []byte {
	var nwSource io.ReadCloser
	if termutil.Isatty(os.Stdin.Fd()) {
		// get from file
		if flag.NArg() != 1 {
			fmt.Println("ERROR: missing network definition file to run")
			printUsage()
		}

		if debug {
			fmt.Println("reading network definition from file", flag.Arg(0))
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
			fmt.Println("found something piped on STDIN, reading network definition from it")
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
	return nwBytes
}

func parseNetworkDefinition(nwBytes []byte, debug bool) *fbp.Fbp {
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
	return nw
}

func displayNetworkDefinition(nw *fbp.Fbp) {
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

func networkDefinition2Processes(nw *fbp.Fbp, debug bool) Network {
	// prepare list of processes
	procs := make(Network)
	for _, fbpProc := range nw.Processes {
		proc := newProcess(fbpProc)
		if _, exists := procs[proc.Name]; exists {
			// error case
			fmt.Println("ERROR: a process already exists by that name:", proc.Name)
			os.Exit(1)
		}
		procs[proc.Name] = proc
	}

	// add connections
	if debug {
		fmt.Println("network:")
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
			fmt.Printf("  inport (.fbp): %s -> %s.%s\n", name, iport.Process, iport.Port)
		}

		// prepare connection data
		toProc := iport.Process
		toPort := iport.Port
		fromPort := name
		/*
			if _, exists := inEndpoints[fromPort]; !exists {
				// no endpoint address was given for that network inport
				fmt.Println("ERROR: no endpoint address given resp. missing -in argument for inport", name)
				os.Exit(2)
			}
		*/

		// listen input port struct
		// NOTE: resets the portname, will be added later.
		//TODO decide if internal or external port name should be used
		/*
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
		*/
		procs[toProc].InPorts = append(procs[toProc].InPorts, Port{
			LocalPort:  toPort,
			RemotePort: fromPort,
			RemoteProc: "NETIN", //TODO
		})

		// destination info
		if debug {
			fmt.Printf("  inport: %s at %s -> %s.%s\n", name, toPort, iport.Process, iport.Port)
		}
	}
	// add regular internal connections
	for _, fbpConn := range nw.Connections {
		if debug {
			fmt.Printf("  connection (.fbp): source=%s, target=%s, data=%s\n", fbpConn.Source, fbpConn.Target, fbpConn.Data)
		}

		if fbpConn.Source != nil && fbpConn.Target != nil { // regular connection
			// prepare connection data
			fromPort := generatePortName(fbpConn.Source)
			toPort := generatePortName(fbpConn.Target)
			fromProc := fbpConn.Source.Process
			toProc := fbpConn.Target.Process

			// connecting output port struct
			procs[fromProc].OutPorts = append(procs[fromProc].OutPorts, Port{
				LocalPort:  fromPort,
				RemotePort: toPort,
				RemoteProc: toProc,
			})

			// listen input port struct
			procs[toProc].InPorts = append(procs[toProc].InPorts, Port{
				LocalPort:  toPort,
				RemotePort: fromPort,
				RemoteProc: fromProc,
			})

			if debug {
				fmt.Printf("  connection: %s.%s -> %s.%s\n", fromProc, fromPort, toProc, toPort)
			}
		} else if fbpConn.Data != "" { // source is IIP
			// prepare connection data
			toPort := generatePortName(fbpConn.Target)
			toProc := fbpConn.Target.Process

			// listen input port struct
			procs[toProc].IIPs = append(procs[toProc].IIPs, IIP{toPort, fbpConn.Data})

			if debug {
				fmt.Printf("  connection: IIP '%s' -> %s.%s\n", fbpConn.Data, toProc, toPort)
			}
		} else if fbpConn.Source == nil { // error condition
			// NOTE: network inports are given separately in nw.Inports
			fmt.Println("ERROR: connection has empty connection source:", fbpConn.String())
			os.Exit(2)
		} else if fbpConn.Target == nil { // error condition
			// NOTE: network outports are given separately in nw.Outports
			fmt.Println("ERROR: connection has empty connection target:", fbpConn.String())
			os.Exit(2)
		}
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

		// connecting output port struct
		procs[fromProc].OutPorts = append(procs[fromProc].OutPorts, Port{
			LocalPort:  fromPort,
			RemotePort: toPort,
			RemoteProc: "NETOUT", //TODO
		})

		// destination info
		if debug {
			fmt.Printf("  outport: %s.%s -> %s\n", oport.Process, oport.Port, name)
		}
	}

	// return process list
	return procs
}

func newProcess(proc *fbp.Process) *Process {
	// return new Process struct
	return &Process{Path: proc.Component, Name: proc.Name, InPorts: []Port{}, OutPorts: []Port{}, IIPs: []IIP{}}
}

func generatePortName(endpoint *fbp.Endpoint) string {
	if endpoint.Index == nil {
		return endpoint.Port
	}
	return fmt.Sprintf("%s[%d]", endpoint.Port, *endpoint.Index)
}
