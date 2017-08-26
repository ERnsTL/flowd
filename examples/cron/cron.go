package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/gorhill/cronexpr"
	shellquote "github.com/kballard/go-shellquote"
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
		Port:     "",
	}
)

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout = bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// flag variables
	var debug, quiet bool
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", netin); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		// parse IIP
		var when whenFlag
		var to toFlag
		// split into arguments, respecting quoted multi-word arguments
		iipSplit, err := shellquote.Split(iip)
		if err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: parsing IIP:", err)
			os.Exit(2)
		}
		flags := flag.NewFlagSet(os.Args[0], flag.ContinueOnError)
		flags.Var(&when, "when", "cron expression when to send IP")
		flags.Var(&to, "to", "port to send IP from for preceding -when")
		flags.BoolVar(&debug, "debug", false, "give detailed event output")
		flags.BoolVar(&quiet, "quiet", false, "no informational output except errors")
		if err := flags.Parse(iipSplit); err != nil {
			os.Exit(2)
		}

		// check flags
		if flags.NArg() != 0 {
			fmt.Fprintln(os.Stderr, "ERROR: unexpected free argument(s) encountered:", flags.Args())
			printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
		if whenTemp != nil {
			fmt.Fprintln(os.Stderr, "ERROR: -when without following -to, but both required")
			printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
		if len(entries) == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: missing cron expression(s) in -when")
			printUsage()
			flags.PrintDefaults() // prints to STDERR
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
	}

	// handle events
	// NOTE: this could also be done using a Goroutine handler per schedule or time.After,
	//	but these solutions would not guarantee event ordering on same time,
	//	e.g. multiple @midnight schedules or similar
	var foundNext bool
	var minIndex int
	var now time.Time
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
				if !quiet {
					fmt.Fprintf(os.Stderr, "WARNING: entry %d has no more future events - disabling entry\n", index+1)
				}
				// mark as unactive
				entries[index].active = false
			}
		}
		if !foundNext {
			// done, exit
			if !quiet {
				fmt.Fprintln(os.Stderr, "all entries have no more future events - exiting.")
			}
			return
		}

		// sleep
		if debug {
			fmt.Fprintln(os.Stderr, "sleeping until next event")
		}
		time.Sleep(time.Until(entries[minIndex].nextEvent))

		// send notification
		//TODO optimize: entry or index as parameter?
		if !quiet {
			fmt.Fprintln(os.Stderr, "woke up, notifying", entries[minIndex].outport)
		}
		sendNotification(minIndex)

		// any active (!) with negative duration = were scheduled for this same time
		now = time.Now()
		for index, entry := range entries {
			if index != minIndex && entry.active && entry.nextEvent.Before(now) {
				// send notification for that also
				if !quiet {
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

	// NOTE: currently no frames expected, but may change in future (shutdown notification etc.)
	/*
		var err error
		var frame *flowd.Frame

		for {
			// read frame
			frame, err = flowd.ParseFrame(netin)
			if err != nil {
				fmt.Fprintln(os.Stderr, err)
			}

			// handle control frames
			if frame.Type == "control" && frame.BodyType == "PortClose" && frame.Port == "IN" {
				fmt.Fprintln(os.Stderr, "input port closed - exiting.")
				// exit
				return
			}

			// anything else is an error since nothing expected
			if !quiet {
				fmt.Fprintln(os.Stderr, "WARNING: got unexpected frame with body:", string(frame.Body))
			}
		}
	*/
}

func sendNotification(index int) {
	// send wake-up frame
	wakeupFrame.Port = entries[index].outport
	if err := wakeupFrame.Marshal(netout); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "IIP format: [flags] {-when [cron-expression] -to [output-port]}...")
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
