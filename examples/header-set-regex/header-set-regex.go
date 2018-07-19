package main

import (
	"flag"
	"fmt"
	"io"
	"os"
	"regexp"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
)

func main() {
	// get configuration from flags = Unix IIP
	var field string
	flag.StringVar(&field, "field", "myfield", "header field to set using matched subgroup")
	unixfbp.DefFlags()
	flag.Parse()

	// check flags
	if flag.NArg() == 0 {
		fmt.Fprintln(os.Stderr, "ERROR: missing regular expression")
		printUsage()
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	} else if flag.NArg() != 1 {
		fmt.Fprintln(os.Stderr, "ERROR: only 1 free argument expected")
		printUsage()
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	if !unixfbp.Quiet {
		fmt.Fprintf(os.Stderr, "got %d modification specification(s)\n", flag.NArg())
	}

	// compile regular expression
	exp, err := regexp.Compile(flag.Arg(0))
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: regular expression does not compile:", flag.Arg(0))
		os.Exit(2)
	} else if unixfbp.Debug {
		fmt.Fprintln(os.Stderr, "got regular expression:", flag.Arg(0))
	}

	if !unixfbp.Quiet {
		fmt.Fprintln(os.Stderr, "reading packets")
	}

	// connect to FBP network
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
	var matches [][]byte

	// read frames
	for {
		frame, err = flowd.Deserialize(netin)
		if err != nil {
			if err == io.EOF {
				break
			}
			fmt.Fprintln(os.Stderr, err)
			break
		}

		// check for match
		matches = exp.FindSubmatch(frame.Body) //TODO [0] = match of entire expression, [1..] = submatches / match groups
		if matches == nil {
			// no match, forward unmodified
			if unixfbp.Debug {
				fmt.Fprintln(os.Stderr, "no match, forwarding unmodified:", string(frame.Body))
			} else if !unixfbp.Quiet {
				fmt.Fprintln(os.Stderr, "no match, forwarding unmodified")
			}
		} else {
			if unixfbp.Debug {
				fmt.Fprintf(os.Stderr, "got matches, modifying: %v\n", matches)
			} else if !unixfbp.Quiet {
				fmt.Fprintf(os.Stderr, "match found, setting %s to %s\n", field, string(matches[1]))
			}

			// modify frame
			if frame.Extensions == nil {
				if unixfbp.Debug {
					fmt.Fprintln(os.Stderr, "frame extensions map is nil, initalizing")
				}
				frame.Extensions = map[string]string{}
			}
			frame.Extensions[field] = string(matches[1])
		}

		// send out modified frame
		//frame.Port = "OUT"
		if err = frame.Serialize(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
		}
		if netin.Buffered() == 0 {
			if err = netout.Flush(); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
			}
		}
	}

}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Arguments: [flags] -field [header-field] [reg-exp]")
	fmt.Fprintln(os.Stderr, "expression is free argument may contain space, do not quote")
}
