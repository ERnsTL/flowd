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

func main() {
	// open connection to network
	bufr := bufio.NewReader(os.Stdin)
	// flag variables
	var debug, quiet bool
	var brackets, control bool
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", bufr); err != nil {
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

	var frame *flowd.Frame //TODO why is this pointer to Frame?
	var err error

	for {
		// read frame
		frame, err = flowd.ParseFrame(bufr)
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
					os.Exit(0)
				}
			}
		} else {
			packetCount++
			packetSize += len(frame.Body)
			//TODO if debug
			//fmt.Fprintf(os.Stderr, "increased packetCount to %d\n", packetCount)
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
)

func sendCount() {
	//TODO if debug fmt.Fprintf(os.Stderr, "sending packet count of %d\n", packetCount)
	// send count to output port
	if packets {
		countFrame.Body = []byte(strconv.Itoa(packetCount))
	} else {
		countFrame.Body = []byte(strconv.Itoa(packetSize))
	}
	if err := countFrame.Marshal(os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
	}
}
