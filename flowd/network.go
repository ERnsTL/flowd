package main

import (
	"flag"
	"fmt"
	"io"
	"io/ioutil"
	"os"
	"strings"

	"github.com/ERnsTL/flowd/flowd/drawfbp"
	"github.com/ERnsTL/flowd/flowd/noflo"
	termutil "github.com/andrew-d/go-termutil"
	"github.com/oleksandr/fbp"
)

// The Network type holds a map of processes
type Network map[string]*Process

// Process holds information about a network process
//TODO optimize: small optimization; instead of string maps -> int32 using symbol table, see https://syslog.ravelin.com/making-something-faster-56dd6b772b83
//TODO mixes concern "hold network information" and "hold handy runtime information"
type Process struct {
	Path     string
	Name     string
	InPorts  []Port
	OutPorts []Port
	IIPs     []IIP
	Instance *ComponentInstance
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

func getNetworkDefinition() []byte {
	var nwSource io.ReadCloser
	if flag.NArg() == 1 {
		// get from file
		if debug {
			fmt.Println("reading network definition from file", flag.Arg(0))
		}
		var err error
		nwSource, err = os.Open(flag.Arg(0))
		if err != nil {
			fmt.Println(err.Error())
			os.Exit(1)
		}
	} else if !termutil.Isatty(os.Stdin.Fd()) {
		// get from STDIN
		if debug {
			fmt.Println("found something piped on STDIN, reading network definition from it")
		}
		nwSource = os.Stdin
	} else {
		fmt.Println("ERROR: missing network definition file to run")
		printUsage()
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

func parseNetworkDefinition(nwBytes []byte) *fbp.Fbp {
	//TODO set Subgraph attribute to nwName if flowd is running as a network component -> process names get that as prefix -> solves name clashes
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

func networkDefinition2Processes(nw *fbp.Fbp) Network {
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
			listenAddress := inEndpoints[fromPort].Url.String() //TODO arg->URL->string is unnecessary - actually, only launch needs to really parse it
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

// Parses and converts .drw network definition into internal Network data structure
func drw2Processes(filepath string) (netflowd Network, err error) {
	// load and parse
	netDRW, err := drawfbp.ParseNetwork(filepath)
	if err != nil {
		return nil, fmt.Errorf("parsing network: %s", err)
	}

	// convert to network

	// map .drw ID to component name
	id2name := map[int]string{}
	iips := map[int]string{}
	enclosures := map[int][]drawfbp.SubnetPort{}
	var procName string
	netflowd = Network{}
	for _, block := range netDRW.Blocks {
		// only use components, subnetworks, IIPs and enclosures
		if block.Type == drawfbp.TypeBlock {
			if block.IsSubnet {
				if debug {
					fmt.Printf("subnet: ID=%d Description=%s DiagramFilename=%s\n", block.ID, block.Description, block.DiagramFileName)
				}
				// generate block name
				procName = drwBlockDesc2ProcessName(block.Description)
				// map block ID -> flowd process name
				id2name[block.ID] = procName
				// convert .drw block to flowd process
				//TODO put this into own function drwNewProcess(block drawfbp.Block)
				//TODO add checks if a process by that name already exists (flowd currently uses the process name as the unique key/ID)
				//TODO add Name/Description to error messages
				if block.DiagramFileName == "" {
					return nil, fmt.Errorf("subnet ID=%d: property DiagramFileName empty", block.ID)
				}
				netflowd[procName] = &Process{
					Path:     "bin/flowd",
					Name:     procName,
					InPorts:  []Port{},
					OutPorts: []Port{},
					IIPs: []IIP{
						IIP{
							Port: "ARGS",
							Data: block.DiagramFileName,
						},
					},
				}
			} else {
				if debug {
					fmt.Printf("component: ID=%d Description=%s CodeFilename=%s BlockClassName=%s\n", block.ID, block.Description, block.CodeFilename, block.BlockClassName)
				}
				//TODO optimize - repetetive
				// generate block name
				procName = drwBlockDesc2ProcessName(block.Description)
				// map block ID -> flowd process name
				id2name[block.ID] = procName
				// convert .drw block to flowd process
				//TODO put this into own function drwNewProcess(block drawfbp.Block)
				//TODO add component name/description to error messages
				if block.CodeFilename == "" && block.BlockClassName == "" {
					return nil, fmt.Errorf("component ID=%d: both properties CodeFilename and BlockClassname empty; one needs to contain component executable path", block.ID)
				}
				netflowd[procName] = &Process{
					Path:     drwOr(block.CodeFilename, block.BlockClassName), // either can be used with preference for CodeFilename
					Name:     procName,
					InPorts:  []Port{},
					OutPorts: []Port{},
					IIPs:     []IIP{},
				}
			}
		} else if block.Type == drawfbp.TypeIIP {
			if debug {
				fmt.Printf("IIP: ID=%d Data=%s\n", block.ID, block.Description)
			}
			// save IIP for now; data will be put into the according flowd process struct during the connections phase
			iips[block.ID] = block.Description
		} else if block.Type == drawfbp.TypeEnclosure {
			if debug {
				fmt.Printf("enclosure: ID=%d Description=%s SubnetPorts=%v\n", block.ID, block.Description, block.SubnetPorts)
			}
			// enclosure inports and outports are just connection forwards; save these
			enclosures[block.ID] = block.SubnetPorts
		} else {
			//fmt.Printf("(ignored) block: ID=%d Type=%s Description=%s\n", block.ID, block.Type, block.Description)
		}
	}

	// add connections incl. enclosure port forwards and IIPs
	// TODO think about removing the enclosures variable - since all connections go through the enclosure border directly into the process, the enclosure port is irrelevant
	var proc *Process
	for _, connection := range netDRW.Connections {
		if debug {
			fmt.Printf("connection: ID %d port %s -> ID %d port %s\n", connection.FromID, connection.UpstreamPort, connection.ToID, connection.DownstreamPort)
		}
		// find source block ID by block type; component, IIP or enclosure is allowed as source
		if fromProcName, fromProcess := id2name[connection.FromID]; fromProcess {
			// find destination block ID by block type; component to either component or enclosure is allowed
			//TODO optimize code - repetetive between the three possibilities
			if toProcName, toProcess := id2name[connection.ToID]; toProcess {
				// connection process -> process

				// create outport at source process
				proc = netflowd[fromProcName]
				proc.OutPorts = append(proc.OutPorts, Port{
					LocalPort:  connection.UpstreamPort,
					RemotePort: connection.DownstreamPort,
					RemoteProc: toProcName,
				})
				// create inport at destination process
				proc = netflowd[toProcName]
				proc.InPorts = append(proc.InPorts, Port{
					LocalPort:  connection.DownstreamPort,
					RemotePort: connection.UpstreamPort,
					RemoteProc: fromProcName,
				})

				//} else if toEncl, toEnclosure := enclosures[connection.ToID]; toEnclosure {
			} else if _, toEnclosure := enclosures[connection.ToID]; toEnclosure {
				// connection process -> enclosure; into or out of enclosure possible

				// NOTE: would have to resolve the process going inside; difficult to differentiate between encl -> proc as being out of enclosure or into it
				// would need preparatory pass over all connections involving enclosures for hop resolution

				// NOTE: special case: double hop, eg. process inside an enclosure -> enclosure outport -> enclosure inport -> process inside other enclosure

				return nil, fmt.Errorf("connection ID=%d: the destination ID=%d in an enclosure - unimplemented; make connection directly to inside process and set subnet port", connection.ID, connection.ToID)
			} else {
				return nil, fmt.Errorf("connection ID=%d: the destination ID=%d is neither component nor enclosure - or does not even exist", connection.ID, connection.ToID)
			}

		} else if iipData, fromIIP := iips[connection.FromID]; fromIIP {
			// find destination block ID by block type; IIP to either component or enclosure is allowed
			if toProcName, toProcess := id2name[connection.ToID]; toProcess {
				// create inport at destination process
				proc = netflowd[toProcName]
				proc.IIPs = append(proc.IIPs, IIP{connection.DownstreamPort, iipData})
				//} else if toEncl, toEnclosure := enclosures[connection.ToID]; toEnclosure {
			} else if _, toEnclosure := enclosures[connection.ToID]; toEnclosure {
				// NOTE: unimplemented
				return nil, fmt.Errorf("connection ID=%d: the destination ID=%d is an enclosure - unimplemented; make connection directly to inside process and set subnet port", connection.ID, connection.ToID)
			} else {
				return nil, fmt.Errorf("connection ID=%d: the destination ID=%d is neither component nor enclosure - or does not even exist", connection.ID, connection.ToID)
			}

			//} else if fromEncl, fromEnclosure := enclosures[connection.FromID]; fromEnclosure {
		} else if _, fromEnclosure := enclosures[connection.FromID]; fromEnclosure {
			// NOTE: unimplemented
			return nil, fmt.Errorf("connection ID=%d: the source ID=%d is an enclosure - unimplemented; make connection directly from inside process and set subnet port", connection.ID, connection.FromID)
			/*
				// find destination block ID by block type; enclosure to either component or enclosure is allowed
				if toProcName, toProcess := id2name[connection.ToID]; toProcess {
					// NOTE: do nothing - tracing of the enclosure hop is being done in forward manner, thus if the destination block is an enclosure; would otherwise generate duplicate connections
					fmt.Println("ignore:", toProcName, toProcess)
				} else if toEncl, toEnclosure := enclosures[connection.ToID]; toEnclosure {
					// NOTE: do nothing - see comment above
					fmt.Println("ignore:", toEncl, toEnclosure)
				} else {
					return nil, fmt.Errorf("connection ID=%d: the destination ID=%d is neither component nor enclosure - or does not even exist", connection.ID, connection.ToID)
				}
			*/

		} else {
			return nil, fmt.Errorf("connection %d: the source ID=%d is neither component, IIP nor enclosure - or does not even exist", connection.ID, connection.FromID)
		}
	}

	return
}

// sanitize description
func drwBlockDesc2ProcessName(desc string) string {
	return strings.Replace(strings.TrimSpace(desc), " ", "_", -1)
}

func drwOr(str1 string, str2 string) string {
	if str1 != "" {
		return str1
	}
	return str2
}

// Parses and converts JSON network definition into internal Network data structure
func json2Processes(filepath string) (netflowd Network, err error) {
	// load and parse
	netJSON, err := noflo.ParseNetwork(filepath)
	if err != nil {
		return nil, fmt.Errorf("parsing network: %s", err)
	}

	// convert to network

	// convert inports, outports, processes, components and connections
	for _ = range netJSON.Connections {
		//TODO
	}

	// TODO
	return nil, fmt.Errorf("unimplemented yet")
}
