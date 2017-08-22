package main

import (
	"bufio"
	"bytes"
	"flag"
	"fmt"
	"os"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// flag variables
	var filterStrings []string
	var debug, quiet, pass, drop, and, or bool
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", netin); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		// parse IIP
		flags := flag.NewFlagSet("packet-filter-string", flag.ContinueOnError)
		flags.BoolVar(&debug, "debug", false, "give detailed event output")
		flags.BoolVar(&quiet, "quiet", false, "no informational output except errors")
		flags.BoolVar(&and, "and", false, "all given substrings must be present in packet body")
		flags.BoolVar(&or, "or", false, "any of the given substrings must be present in packet body")
		flags.BoolVar(&pass, "pass", false, "let matching packets pass")
		flags.BoolVar(&drop, "drop", false, "drop matching packets")
		if err := flags.Parse(strings.Split(iip, " ")); err != nil {
			os.Exit(2)
		}
		if (!and && !or) || (and && or) {
			if flags.NArg() == 1 {
				// do not annoy user if only one substring -> set to or (or and)
				and = false
				or = true
			} else {
				fmt.Fprintln(os.Stderr, "ERROR: either -and or -or expected - unable to proceed")
				printUsage()
				flags.PrintDefaults() // prints to STDERR
				os.Exit(2)
			}
		}
		if (!pass && !drop) || (pass && drop) {
			fmt.Fprintln(os.Stderr, "ERROR: either -pass or -drop expected - unable to proceed")
			printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
		if flags.NArg() == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: no filter substrings given")
			printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
		//TODO optimize - convert search substrings to []byte, since frame.Body is []byte
		filterStrings = flags.Args()
	}
	if !quiet {
		fmt.Fprintln(os.Stderr, "starting up")
	}

	// prepare variables
	var frame *flowd.Frame //TODO why is this pointer to Frame?
	var err error

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
		frame, err = flowd.ParseFrame(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// check for closed input port
		if frame.Type == "control" && frame.BodyType == "PortClose" {
			// shut down operations
			fmt.Fprintln(os.Stderr, "received port close notification - exiting.")
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
	fmt.Fprintln(os.Stderr, "IIP format: [-pass|-drop] [-and|-or] [flags] [substring]...")
}

//TODO optimize: give netout as parameter or have it as global variable?
func forwardFrame(frame *flowd.Frame, netout *bufio.Writer) {
	frame.Port = "OUT"
	if err := frame.Marshal(netout); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err)
	}
}
