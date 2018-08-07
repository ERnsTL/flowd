package main

import (
	"bufio"
	"bytes"
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
)

func main() {
	// flag variables
	var filterStrings []string
	var pass, drop, and, or bool
	// get configuration from arguments = Unix IIP
	unixfbp.DefFlags()
	flag.BoolVar(&and, "and", false, "all given substrings must be present in packet body")
	flag.BoolVar(&or, "or", false, "any of the given substrings must be present in packet body")
	flag.BoolVar(&pass, "pass", false, "let matching packets pass")
	flag.BoolVar(&drop, "drop", false, "drop matching packets")
	flag.Parse()

	// check flags
	if (!and && !or) || (and && or) {
		if flag.NArg() == 1 {
			// do not annoy user if only one substring -> set to or (or and)
			and = false
			or = true
		} else {
			fmt.Fprintln(os.Stderr, "ERROR: either -and or -or expected - unable to proceed")
			printUsage()
			flag.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
	}
	if (!pass && !drop) || (pass && drop) {
		fmt.Fprintln(os.Stderr, "ERROR: either -pass or -drop expected - unable to proceed")
		printUsage()
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	if flag.NArg() == 0 {
		fmt.Fprintln(os.Stderr, "ERROR: no filter substrings given")
		printUsage()
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	//TODO optimize - convert search substrings to []byte, since frame.Body is []byte
	filterStrings = flag.Args()

	if !unixfbp.Quiet {
		fmt.Fprintln(os.Stderr, "starting up")
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
	if !unixfbp.Quiet {
		fmt.Fprintln(os.Stderr, "filtering")
	}

	// prepare variables
	var frame *flowd.Frame

	// main work loop
	/* pseudo-code:
	and-pass:
		for:
			if !contains:
				continue nextframe
		forward

	and-drop:
		for:
			if !contains:
				forward
				continue nextframe / break
		continue / continue next frame = drop

	or-pass:
		for:
			if contains:
				forward
				continue nextframe / break
		continue next frame = drop

	or-drop:
		for:
			if contains:
				continue nextframe / break
		forward

	-> group by condition = outer if and
	*/
nextframe:
	for {
		// read frame
		frame, err = flowd.Deserialize(netin)
		if err != nil {
			if err == io.EOF {
				fmt.Fprintln(os.Stderr, "EOF - exiting.")
				break
			}
			fmt.Fprintln(os.Stderr, err)
			break
		}

		// apply filter only to non-bracket IPs
		if !(frame.Type == "control" && (frame.BodyType == "BracketOpen" || frame.BodyType == "BracketClose")) {
			// check conditions on body
			//TODO code could be more readable by using a matched bool variable
			if and {
				// check for substrings in body
				for _, string := range filterStrings {
					if !bytes.Contains(frame.Body, []byte(string)) {
						// and-condition has been failed, act according to target
						if drop {
							// condition for drop failed, thus forward it to output port
							forwardFrame(frame, netout)
							// next frame
							continue nextframe // or break
						} else {
							continue nextframe
						}
					}
				}
				// and-condition has been met, act according to target
				if drop {
					continue nextframe
				} else {
					// nothing to do here, goes on to pass
				}
			} else {
				// check for substrings in body
				for _, string := range filterStrings {
					if bytes.Contains(frame.Body, []byte(string)) {
						// passed, act according to target
						if drop {
							continue nextframe
						} else {
							// condition for pass has been met, thus forward it to output port
							forwardFrame(frame, netout)
							// next frame
							continue nextframe // or break
						}
					}
				}
				// or-condition has been failed, act according to target
				if pass {
					continue nextframe
				} else {
					// nothing to do here, goes on to pass
				}
			}
		}

		// forward it to output port
		forwardFrame(frame, netout)
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Arguments: [-pass|-drop] [-and|-or] [flags] [substring]...")
}

//TODO optimize: give netout as parameter or have it as global variable?
func forwardFrame(frame *flowd.Frame, netout *bufio.Writer) {
	//frame.Port = "OUT"
	if err := frame.Serialize(netout); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err)
	}
}
