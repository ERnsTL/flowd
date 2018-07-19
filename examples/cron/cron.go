package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
	"github.com/gorhill/cronexpr"
)

type entry struct {
	cronExpression *string              // to be parsed
	schedule       *cronexpr.Expression // parsed cron expression
	outport        string               // output port to send notification on
	nextEvent      time.Time
	active         bool // does this schedule have future events?
}

var (
	entries     = []entry{}   // ordered list of entries = cron-expressions and target-output-ports
	whenTemp    *string       // state variable for flag parsing, keeps -when value until following -to
	netout      *bufio.Writer //TODO necessary to keep as global variable?
	wakeupFrame = &flowd.Frame{
		Type:     "data",
		BodyType: "Wakeup",
	}
)

func main() {
	// get configuration from flags = Unix IIP
	var when whenFlag
	var to toFlag
	unixfbp.DefFlags()
	flag.Var(&when, "when", "cron expression when to send IP")
	flag.Var(&to, "to", "port to send IP from for preceding -when")
	flag.Parse()
	// check flags
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "ERROR: unexpected free argument(s) encountered:", flag.Args())
		printUsage()
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	if whenTemp != nil {
		fmt.Fprintln(os.Stderr, "ERROR: -when without following -to, but both required")
		printUsage()
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	if len(entries) == 0 {
		fmt.Fprintln(os.Stderr, "ERROR: missing cron expression(s) in -when")
		printUsage()
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}

	// parse cron expressions
	// NOTE: range over array and array slices is ordered (vs. maps)
	now := time.Now()
	for index, entry := range entries {
		// parse cron expression
		if schedule, err := cronexpr.Parse(*entry.cronExpression); err != nil {
			fmt.Fprintf(os.Stderr, "ERROR: parsing cron expression '%s': %s\n", *entry.cronExpression, err)
			os.Exit(2)
		} else {
			// save schedule
			entries[index].schedule = schedule
			// generate and check for next event time
			entries[index].nextEvent = schedule.Next(now)
			if entries[index].nextEvent.IsZero() {
				// entry has no future events
				fmt.Fprintf(os.Stderr, "ERROR: cron expression '%s' has no future events\n", *entry.cronExpression)
				os.Exit(2)
			}
			// else activate
			entries[index].active = true
			// GC string
			entries[index].cronExpression = nil
		}
	}

	// connect to FBP network
	var err error
	for portName := range unixfbp.OutPorts {
		// open all given ports, assuming they will be needed at some point and so that the other side does not block
		netout, _, err = unixfbp.OpenOutPort(portName)
		if err != nil {
			fmt.Println("ERROR:", err)
			os.Exit(2)
		}
		defer netout.Flush()
	}

	// handle events
	// NOTE: this could also be done using a Goroutine handler per schedule or time.After,
	//	but these solutions would not guarantee event ordering on same time,
	//	e.g. multiple @midnight schedules or similar
	var foundNext bool
	var minIndex int
	for {
		// find nearest next event
		foundNext = false
		now = time.Now()
		for index, entry := range entries {
			// filter for active entries
			if !entry.active {
				continue
			}
			// find next event time
			entries[index].nextEvent = entry.schedule.Next(now)
			// check for minimum
			if !entries[index].nextEvent.IsZero() {
				if !foundNext {
					// found first match
					minIndex = index
					foundNext = true
				} else if entries[index].nextEvent.Before(entries[minIndex].nextEvent) {
					// found a match with earlier next event
					minIndex = index
					foundNext = true
				}
			} else {
				if !unixfbp.Quiet {
					fmt.Fprintf(os.Stderr, "WARNING: entry %d has no more future events - disabling entry\n", index+1)
				}
				// mark as unactive
				entries[index].active = false
			}
		}
		if !foundNext {
			// done, exit
			if !unixfbp.Quiet {
				fmt.Fprintln(os.Stderr, "all entries have no more future events - exiting.")
			}
			return
		}

		// sleep
		if unixfbp.Debug {
			fmt.Fprintln(os.Stderr, "sleeping until next event")
		}
		time.Sleep(time.Until(entries[minIndex].nextEvent))

		// send notification
		//TODO optimize: entry or index as parameter?
		if !unixfbp.Quiet {
			fmt.Fprintln(os.Stderr, "woke up, notifying", entries[minIndex].outport)
		}
		sendNotification(minIndex)

		// any active (!) with negative duration = were scheduled for this same time
		now = time.Now()
		for index, entry := range entries {
			if index != minIndex && entry.active && entry.nextEvent.Before(now) {
				// send notification for that also
				if !unixfbp.Quiet {
					fmt.Fprintln(os.Stderr, "also notifying", entries[index].outport)
				}
				sendNotification(index)
			}
		}

		// flush so that they get delivered now
		if err := netout.Flush(); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: flushing net out:", err.Error())
		}
	}
}

func sendNotification(index int) {
	// send wake-up frame
	//wakeupFrame.Port = entries[index].outport
	if err := wakeupFrame.Serialize(netout); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Arguments: [flags] {-when [cron-expression] -to [output-port]}...")
	fmt.Fprintln(os.Stderr, "multiple when+to possible; expression format at https://github.com/gorhill/cronexpr")
}

// flag acceptors of -when [cron-expression] -to [output-port] couples
// NOTE: need two separate in order to know that value came from which flag
// NOTE: similar to flag types in packet-router-header component
type whenFlag struct{}

func (f *whenFlag) String() string {
	// return default value
	return ""
}

func (f *whenFlag) Set(value string) error {
	if whenTemp == nil {
		// -when flag given, fill it
		whenTemp = &value
		return nil
	}
	// -when flag was previously given, but -to expected, not another -when
	return fmt.Errorf("-when followed by -when, expected -to")
}

type toFlag struct{}

func (f *toFlag) String() string {
	// return default value
	return ""
}

func (f *toFlag) Set(value string) error {
	if whenTemp == nil {
		// no value from previous -when flag
		return fmt.Errorf("-to without preceding -when")
	}
	// all data given for a rule, save it
	entries = append(entries, entry{
		cronExpression: whenTemp,
		outport:        value,
	})
	// reset for next -when -to duet
	whenTemp = nil
	return nil
}
