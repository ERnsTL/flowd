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
	// prepared frame
	countFrame = &flowd.Frame{
		Type:     "data",
		BodyType: "Count",
		Port:     "OUT",
	}
	netout *bufio.Writer //TODO is concurrent use OK in this case here?
)

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout = bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// flag variables
	var brackets, control bool
	var packets, size bool
	var packetsPerFieldValue string

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
		flags.StringVar(&packetsPerFieldValue, "packetsperfieldvalue", "", "count number of packets per value of given header field")
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
		countersGiven := 0
		if size {
			countersGiven++
		}
		if packets {
			countersGiven++
		}
		if packetsPerFieldValue != "" {
			countersGiven++
		}
		if countersGiven != 1 {
			fmt.Fprintln(os.Stderr, "ERROR: either -size, -packets or -packetsperfieldvalue expected - unable to proceed")
			//TODO add this printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
	}

	// prepare according counter
	var counter counter
	if packets {
		counter = &packetCounter{}
	} else if size {
		counter = &sizeCounter{}
	} else if packetsPerFieldValue != "" {
		counter = &packetsPerFieldValueCounter{
			Field:               packetsPerFieldValue,
			countsPerFieldValue: map[string]int{},
		}
	} else {
		fmt.Fprintln(os.Stderr, "ERROR: unknown counter type!")
		os.Exit(2)
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
				counter.Count(frame)
			}
			switch frame.BodyType {
			case "BracketOpen":
				if brackets {
					counter.Reset()
				}
			case "BracketClose":
				if brackets {
					counter.Report()
				}
			case "PortClose":
				if frame.Port == "IN" {
					fmt.Fprintln(os.Stderr, "got port close notification - sending final count before exiting.")
					// send final count
					counter.Report()
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
				counter.Reset()
				continue
			} else if frame.Port == "REPORT" {
				// report current count value
				if !quiet {
					fmt.Fprintln(os.Stderr, "reporting count as requested")
				}
				counter.Report()
				// send immediately; presumably a report is not requested too often
				netout.Flush()
				continue
			}

			// regular case and frame
			counter.Count(frame)
		}
	}
}

type counter interface {
	Count(*flowd.Frame)
	Report()
	Reset()
}

type packetCounter struct {
	packetCount int
}

func (c *packetCounter) Count(frame *flowd.Frame) {
	c.packetCount++
	if !quiet {
		fmt.Fprintln(os.Stderr, "increased packet count to", c.packetCount)
	}
}

func (c *packetCounter) Report() {
	// fill count frame
	if !quiet {
		fmt.Fprintln(os.Stderr, "sending packet count of", c.packetCount)
	}
	countFrame.Body = []byte(strconv.Itoa(c.packetCount))
	// send report to FBP network
	if err := countFrame.Marshal(netout); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
	}
}

func (c *packetCounter) Reset() {
	c.packetCount = 0
}

type sizeCounter struct {
	totalSize int
}

func (c *sizeCounter) Count(frame *flowd.Frame) {
	c.totalSize += len(frame.Body)
	if !quiet {
		fmt.Fprintln(os.Stderr, "increased total packet size to", c.totalSize)
	}
}

func (c *sizeCounter) Report() {
	// fill count frame
	if !quiet {
		fmt.Fprintln(os.Stderr, "sending total size of", c.totalSize)
	}
	countFrame.Body = []byte(strconv.Itoa(c.totalSize))
	// send report to FBP network
	if err := countFrame.Marshal(netout); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
	}
}

func (c *sizeCounter) Reset() {
	c.totalSize = 0
}

type packetsPerFieldValueCounter struct {
	Field               string
	countsPerFieldValue map[string]int
}

func (c *packetsPerFieldValueCounter) Count(frame *flowd.Frame) {
	if frame.Extensions != nil {
		if valueInFrame, exists := frame.Extensions[c.Field]; exists {
			if _, exists := c.countsPerFieldValue[valueInFrame]; exists {
				// increase value
				c.countsPerFieldValue[valueInFrame]++
			} else {
				// make new entry with initial value
				c.countsPerFieldValue[valueInFrame] = 1
			}
			if !quiet {
				fmt.Fprintf(os.Stderr, "increased packet count for %s=%s to %d\n", c.Field, valueInFrame, c.countsPerFieldValue[valueInFrame])
			}
		} else {
			// given field not found in frame, nothing to count
			if !quiet {
				fmt.Fprintln(os.Stderr, c.Field, "not found, count unchanged")
			}
		}
	} else {
		// no extensions found, nothing to count
		if !quiet {
			fmt.Fprintln(os.Stderr, c.Field, "not found, count unchanged")
		}
	}
}

func (c *packetsPerFieldValueCounter) Report() {
	if !quiet {
		fmt.Fprintln(os.Stderr, "sending packet counts per values of field", c.Field)
	}
	for fieldValue, count := range c.countsPerFieldValue {
		if !quiet {
			fmt.Fprintf(os.Stderr, "\t%s=%s appeared %d times\n", c.Field, fieldValue, count)
		}
		// fill count frame
		countFrame.Extensions = map[string]string{
			c.Field: fieldValue,
		}
		///TODO if above works, remove - countFrame.Extensions[c.Field] = fieldValue
		countFrame.Body = []byte(strconv.Itoa(count))
		// send report to FBP network
		if err := countFrame.Marshal(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
		}
	}
}

func (c *packetsPerFieldValueCounter) Reset() {
	c.countsPerFieldValue = map[string]int{}
}
