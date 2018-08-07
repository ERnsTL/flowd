package main

import (
	"bufio"
	"bytes"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
)

//NOTE: this component is currently just a tailored variant of the cmd component
//TODO use a mDNS Go library - already have that in the drawer

//TODO add option to output either as an URL in frame body (tcp4://127.0.0.1:8080) or just host:port or fill the header fields
//TODO add example of browser result to make dynamic connection to network brigde destination

var (
	ipv4, ipv6 bool
	numresults int
)

func main() {
	// get configuration from arguments = Unix IIP
	unixfbp.DefFlags()
	//TODO make wait time configurable
	//TODO finish immediately once number of results has been reached
	flag.BoolVar(&ipv4, "ipv4", true, "return results for IPv4")
	flag.BoolVar(&ipv6, "ipv6", false, "return results for IPv6")
	flag.IntVar(&numresults, "n", 1, "number of results to return")
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "ERROR: unexpected free argument(s) - exiting.")
		printUsage()
		flag.PrintDefaults()
		os.Exit(2)
	}
	// checks
	if numresults > 1 {
		//TODO implement (put into brackets with flag for that)
		fmt.Fprintln(os.Stderr, "ERROR: more than one result currently unimplemented")
		os.Exit(1)
	}
	// connect to FBP network
	var err error
	netin, _, err := unixfbp.OpenInPort("IN")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	netout, _, err := unixfbp.OpenOutPort("OUT")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	defer netout.Flush()

	// main loop
	fmt.Fprintln(os.Stderr, "ready")
	var frame *flowd.Frame
	var browser *exec.Cmd
	outframe := &flowd.Frame{
		Type:     "data",
		BodyType: "MDNSResult",
		//Port:     "OUT",
	}
	ipv4Bytes := []byte{';', 'I', 'P', 'v', '4', ';'}
	ipv6Bytes := []byte{';', 'I', 'P', 'v', '6', ';'}
	sepBytes := []byte{';'}
	for {
		// read frame with the mDNS service type to query for
		frame, err = flowd.Deserialize(netin)
		if err != nil {
			if err == io.EOF {
				// signal for closing
				fmt.Fprintln(os.Stderr, "EOF")
				return
			}
			fmt.Fprintln(os.Stderr, "ERROR:", err)
			break
		}
		if unixfbp.Debug {
			fmt.Fprintln(os.Stderr, "got a request for", string(frame.Body))
		}

		// service type must be a matching service type (main or an additional subtype) including the transport protocol
		// example: _flowd._tcp
		if unixfbp.Debug {
			fmt.Fprintln(os.Stderr, "starting browser")
		}
		const arg0 = "avahi-browse"
		args := []string{"--parsable", string(frame.Body), "--terminate", "--no-db-lookup", "--resolve"}
		if unixfbp.Debug {
			// print command-line
			fmt.Fprintln(os.Stderr, "commandline:", arg0, args)
		}
		browser = exec.Command(arg0, args...)
		out, err := browser.CombinedOutput()
		if unixfbp.Debug {
			fmt.Fprintln(os.Stderr, "output:\n", string(out))
		}
		if err != nil {
			fmt.Println("ERROR:", err)
			os.Exit(4)
		}

		// extract results
		scanner := bufio.NewScanner(bytes.NewReader(out)) //TODO optimize - directly from StdoutPipe
		// scan loop
		//linecount := 0
		for scanner.Scan() {
			// collect line
			outframe.Body = scanner.Bytes()
			//linecount++
			// checks
			if outframe.Body[0] == '+' {
				continue
			} else if outframe.Body[0] == '=' {
				// filter by IP version
				if (!ipv4 && bytes.Contains(outframe.Body, ipv4Bytes)) || (!ipv6 && bytes.Contains(outframe.Body, ipv6Bytes)) {
					//TODO optimize - can probably be optimized for less Contains() checks
					continue
				}
				// extract address and port
				parts := bytes.Split(outframe.Body, sepBytes)
				if len(parts) < 8 {
					fmt.Fprintln(os.Stderr, "ERROR: too few result line fields")
					os.Exit(2)
				}
				outframe.Body = parts[7] //TODO optimize
				outframe.Body = append(outframe.Body, ':')
				outframe.Body = append(outframe.Body, parts[8]...)
				// send it to FBP network
				if err = outframe.Serialize(netout); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: serializing frame:", err)
					os.Exit(1)
				}
				// flush if no more requests waiting
				if netin.Buffered() == 0 {
					if err = netout.Flush(); err != nil {
						fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
					}
				}
				// not more than one result implemented
				break
			} else {
				// some error probably
				//TODO send empty result downstream (in brackets?)
				fmt.Fprintln(os.Stderr, "ERROR:", string(outframe.Body))
				os.Exit(2)
			}
		}

		// check for error
		if scanner.Err() != nil {
			fmt.Fprintln(os.Stderr, "ERROR: scanning for line: "+scanner.Err().Error())
			os.Exit(2)
		} else if !unixfbp.Quiet {
			//TODO fmt.Fprintf(os.Stderr, "finished at %d results total\n", linecount)
		}

		// empty result/error result -> TODO send that downstream
	}
}

func checkError(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR:", err)
		os.Exit(2)
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Arguments: [-debug] [-quiet]; send requests on port IN")
}
