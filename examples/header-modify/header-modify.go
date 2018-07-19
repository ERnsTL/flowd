package main

import (
	"flag"
	"fmt"
	"os"
	"regexp"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
)

func main() {
	// get configuration from flags = Unix IIP
	unixfbp.DefFlags()
	flag.Parse()

	// check flags
	if flag.NArg() == 0 {
		fmt.Fprintln(os.Stderr, "ERROR: missing modification specification(s) in free argument(s)")
		printUsage()
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	if !unixfbp.Quiet {
		fmt.Fprintf(os.Stderr, "got %d modification specification(s)\n", flag.NArg())
	}

	// parse specifications
	type modFunc func(*flowd.Frame)
	modifications := []modFunc{}
	//TODO add ability to set BodyType non-extension field
	//TODO add ability to set field to ""
	//TODO add ability to delete header field
	//TODO maybe add alternative append and prepend operator in case + needs to be in the value
	// NOTE: U: = non-capturing flag "un-greedy"
	re := regexp.MustCompile("(?U:(.+))(\\+=|=\\+|=)(.*)")
	for _, arg := range flag.Args() {
		if unixfbp.Debug {
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
		if unixfbp.Debug {
			fmt.Fprintln(os.Stderr, "parsed argument into specification:", spec)
		}
		switch op {
		case "=":
			// set value; add according function to modification list
			if unixfbp.Debug {
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
				if !unixfbp.Quiet {
					fmt.Fprintf(os.Stderr, "\tsetting %s to %s\n", field, value)
				}
			})
		case "+=":
			// append value, if field exists
			if unixfbp.Debug {
				fmt.Fprintf(os.Stderr, "\tappend %s to %s\n", value, field)
			}
			modifications = append(modifications, func(frame *flowd.Frame) {
				if frame.Extensions != nil {
					if _, found := frame.Extensions[field]; found {
						if !unixfbp.Quiet {
							fmt.Fprintf(os.Stderr, "\tappending %s to %s\n", value, field)
						}
						frame.Extensions[field] += value
						return
					}
				}
				if !unixfbp.Quiet {
					fmt.Fprintf(os.Stderr, "\t%s missing, leaving unmodified\n", field)
				}
			})
		case "=+":
			// prepend, if field exists
			if unixfbp.Debug {
				fmt.Fprintf(os.Stderr, "\tprepend %s to %s\n", value, field)
			}
			modifications = append(modifications, func(frame *flowd.Frame) {
				if frame.Extensions != nil {
					if currentValue, found := frame.Extensions[field]; found {
						if !unixfbp.Quiet {
							fmt.Fprintf(os.Stderr, "\tprepending %s to %s\n", value, field)
						}
						frame.Extensions[field] = value + currentValue
						return
					}
				}
				if !unixfbp.Quiet {
					fmt.Fprintf(os.Stderr, "\t%s missing, leaving unmodified\n", field)
				}
			})
		default:
			fmt.Fprintln(os.Stderr, "ERROR: unexpected operation:", op, "- Exiting.")
			os.Exit(2)
		}
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

	// prepare variables
	var frame *flowd.Frame

	// main loop
	for {
		// read frame
		frame, err = flowd.Deserialize(netin)
		if err != nil {
			//TODO handle EOF gracefully
			fmt.Fprintln(os.Stderr, err)
			break
		}

		// apply modification(s)
		if !unixfbp.Quiet {
			fmt.Fprintln(os.Stderr, "got frame, modifying...")
		}
		for _, ruleFunc := range modifications {
			ruleFunc(frame)
		}
		if !unixfbp.Quiet {
			fmt.Fprintln(os.Stderr, "modified. forwarding now.")
		}

		// send it to given output ports
		//frame.Port = "OUT"
		if netin.Buffered() == 0 {
			if err = frame.Serialize(netout); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
			}
		}
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Arguments: [field][=|+=|=+][value]...")
	fmt.Fprintln(os.Stderr, "= sets, += appends, =+ prepends the value to the given field")
	fmt.Fprintln(os.Stderr, "multiple specifications possible, order guaranteed; possible to quote a specification")
}
