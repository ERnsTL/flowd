package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"regexp"

	"github.com/ERnsTL/flowd/libflowd"
	shellquote "github.com/kballard/go-shellquote"
)

//const maxFlushWait = 100 * time.Millisecond // flush any buffered outgoing frames after at most this duration

func main() {
	// connect to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// flush netout after x seconds if there is buffered data
	// NOTE: bufio.Writer.Write() flushes on its own if buffer is full
	/*
		go func() {
			for {
				time.Sleep(maxFlushWait)
				// NOTE: Flush() checks on its own if data buffered
				netout.Flush()
			}
		}()
	*/
	// get configuration from IIP = initial information packet/frame
	type modFunc func(*flowd.Frame)
	modifications := []modFunc{}
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", netin); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		// split into arguments, respecting quoted multi-word arguments
		iipSplit, err := shellquote.Split(iip)
		if err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: parsing IIP:", err)
			os.Exit(2)
		}
		var debug, quiet bool
		flags := flag.NewFlagSet(os.Args[0], flag.ContinueOnError)
		flags.BoolVar(&debug, "debug", false, "give detailed event output")
		flags.BoolVar(&quiet, "quiet", false, "no informational output except errors")
		if err := flags.Parse(iipSplit); err != nil {
			os.Exit(2)
		}

		// check flags
		if flags.NArg() == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: missing modification specification(s) in free argument(s)")
			printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
		if !quiet {
			fmt.Fprintf(os.Stderr, "got %d modification specification(s)\n", flags.NArg())
		}

		// parse specifications
		//TODO add ability to set BodyType non-extension field
		//TODO add ability to set field to ""
		//TODO add ability to delete header field
		//TODO maybe add alternative append and prepend operator in case + needs to be in the value
		// NOTE: U: = non-capturing flag "un-greedy"
		re := regexp.MustCompile("(?U:(.+))(\\+=|=\\+|=)(.*)")
		for _, arg := range flags.Args() {
			if debug {
				fmt.Fprintln(os.Stderr, "got argument:", arg)
			}
			spec := re.FindStringSubmatch(arg)
			if spec == nil {
				// no match, display usage information
				printUsage()
				os.Exit(2)
			}
			field := spec[1]
			op := spec[2]
			value := spec[3]
			if debug {
				fmt.Fprintln(os.Stderr, "parsed argument into specification:", spec)
			}
			switch op {
			case "=":
				// set value; add according function to modification list
				if debug {
					fmt.Fprintf(os.Stderr, "\tset %s to %s\n", field, value)
				}
				modifications = append(modifications, func(frame *flowd.Frame) {
					if frame.Extensions == nil {
						frame.Extensions = map[string]string{
							field: value,
						}
					} else {
						frame.Extensions[field] = value
					}
					/*TODO optimize: which version is more performant? beter for CPU branch predictor?
					if frame.Extensions == nil {
						frame.Extensions = map[string]string{}
					}
					frame.Extensions[field] = value
					*/
					fmt.Fprintf(os.Stderr, "\tsetting %s to %s\n", field, value)
				})
			case "+=":
				// append value, if field exists
				if debug {
					fmt.Fprintf(os.Stderr, "\tappend %s to %s\n", value, field)
				}
				modifications = append(modifications, func(frame *flowd.Frame) {
					if frame.Extensions != nil {
						if _, found := frame.Extensions[field]; found {
							fmt.Fprintf(os.Stderr, "\tappending %s to %s\n", value, field)
							frame.Extensions[field] += value
							return
						}
					}
					fmt.Fprintf(os.Stderr, "\t%s missing, leaving unmodified\n", field)
				})
			case "=+":
				// prepend, if field exists
				if debug {
					fmt.Fprintf(os.Stderr, "\tprepend %s to %s\n", value, field)
				}
				modifications = append(modifications, func(frame *flowd.Frame) {
					if frame.Extensions != nil {
						if currentValue, found := frame.Extensions[field]; found {
							fmt.Fprintf(os.Stderr, "\tprepending %s to %s\n", value, field)
							frame.Extensions[field] = value + currentValue
							return
						}
					}
					fmt.Fprintf(os.Stderr, "\t%s missing, leaving unmodified\n", field)
				})
			default:
				fmt.Fprintln(os.Stderr, "ERROR: unexpected operation:", op, "- Exiting.")
				os.Exit(2)
			}
		}
	}

	// prepare variables
	var frame *flowd.Frame
	var err error

	// main loop
	for {
		// read frame
		frame, err = flowd.ParseFrame(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// check for port closed
		if frame.Type == "control" && frame.BodyType == "PortClose" && frame.Port == "IN" {
			fmt.Fprintln(os.Stderr, "input port closed - exiting.")
			break
		}

		// apply modification(s)
		//TODO make this configurable using "debug" parameter in IIP
		fmt.Fprintln(os.Stderr, "got frame, modifying...")
		for _, ruleFunc := range modifications {
			ruleFunc(frame)
		}
		fmt.Fprintln(os.Stderr, "modified. forwarding now.")

		// send it to given output ports
		frame.Port = "OUT"
		if err = frame.Marshal(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
		}
		if err = netout.Flush(); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
		}
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "IIP format: [field][=|+=|=+][value]...")
	fmt.Fprintln(os.Stderr, "= sets, += appends, =+ prepends the value to the given field")
	fmt.Fprintln(os.Stderr, "multiple specifications possible, order guaranteed; possible to quote a specification")
}
