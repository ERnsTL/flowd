package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"strconv"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
)

var (
	debug, quiet bool
)

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout = bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// flag variables
	var brackets, control bool
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", netin); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		// parse IIP
		flags := flag.NewFlagSet("counter", flag.ContinueOnError)
		flags.BoolVar(&brackets, "brackets", false, "expect bracketed input streams")
		flags.BoolVar(&control, "control", false, "count control packets as well")
		flags.BoolVar(&size, "size", false, "count size of packet bodies")
		flags.BoolVar(&packets, "packets", false, "count number of packets")
		flags.BoolVar(&debug, "debug", false, "give detailed event output")
		flags.BoolVar(&quiet, "quiet", false, "no informational output except errors")
		if err := flags.Parse(strings.Split(iip, " ")); err != nil {
			os.Exit(2)
		}
		if flags.NArg() != 0 {
			fmt.Fprintln(os.Stderr, "ERROR: unexpected free arguments given")
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
		if (!size && !packets) || (size && packets) {
			fmt.Fprintln(os.Stderr, "ERROR: either -size or -packets expected - unable to proceed")
			//TODO add this printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
	}

	var frame *flowd.Frame
	var err error

	for {
		// read frame
		frame, err = flowd.ParseFrame(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// handle control frames
		if frame.Type == "control" {
			if control {
				packetCount++
				packetSize += len(frame.Body)
			}
			switch frame.BodyType {
			case "BracketOpen":
				if brackets {
					packetCount = 0
				}
			case "BracketClose":
				if brackets {
					sendCount()
				}
			case "PortClose":
				if frame.Port == "IN" {
					fmt.Fprintf(os.Stderr, "got port close notification - sending final count before exiting.")
					// send final count
					sendCount()
					// exit
					// NOTE: os.Exit() would prevent the deferred netout.Flush()
					return
				}
			}
		} else {
			// check for special requests
			if frame.Port == "RESET" {
				// reset counts
				if !quiet {
					fmt.Fprintln(os.Stderr, "resetting count as requested")
				}
				packetCount = 0
				packetSize = 0
				continue
			} else if frame.Port == "REPORT" {
				// report current count value
				if !quiet {
					fmt.Fprintln(os.Stderr, "reporting count as requested")
				}
				sendCount()
				netout.Flush()
				continue
			}

			// regular case and frame
			packetCount++
			packetSize += len(frame.Body)
			if debug {
				fmt.Fprintf(os.Stderr, "increased packet count to %d, size count to %d\n", packetCount, packetSize)
			}
		}
	}
}

// NOTE: used for optimal (? TODO) transfer of information to sendCount
var (
	packets     bool
	size        bool
	packetCount int
	packetSize  int
	countFrame  = &flowd.Frame{
		Type:     "data",
		BodyType: "PacketCount",
		Port:     "OUT",
	}
	netout *bufio.Writer //TODO is concurrent use OK in this case here?
)

func sendCount() {
	// send count to output port
	if packets {
		if debug {
			fmt.Fprintf(os.Stderr, "sending packet count of %d\n", packetCount)
		}
		countFrame.Body = []byte(strconv.Itoa(packetCount))
	} else {
		if debug {
			fmt.Fprintf(os.Stderr, "sending size count of %d\n", packetSize)
		}
		countFrame.Body = []byte(strconv.Itoa(packetSize))
	}
	if err := countFrame.Marshal(netout); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
	}
}
