package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"os"
	"regexp"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
)

//const maxFlushWait = 100 * time.Millisecond // flush any buffered outgoing frames after at most this duration

func main() {
	// open connection to network
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
	var exp *regexp.Regexp
	var field string
	var debug, quiet bool
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", netin); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		// parse IIP
		flags := flag.NewFlagSet(os.Args[0], flag.ContinueOnError)
		flags.StringVar(&field, "field", "myfield", "header field to set using matched subgroup")
		flags.BoolVar(&debug, "debug", false, "give detailed event output")
		flags.BoolVar(&quiet, "quiet", false, "no informational output except errors")
		if err = flags.Parse(strings.Split(iip, " ")); err != nil {
			os.Exit(2)
		}
		if flags.NArg() == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: missing regular expression")
			printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
		// compile regular expression
		regexpStr := strings.Join(flags.Args(), " ") // in case regular expression contains space
		exp, err = regexp.Compile(regexpStr)
		if err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: regular expression does not compile:", regexpStr)
			os.Exit(2)
		} else if debug {
			fmt.Fprintln(os.Stderr, "got regular expression:", regexpStr)
		}
	}
	if !quiet {
		fmt.Fprintln(os.Stderr, "reading packets")
	}

	var frame *flowd.Frame
	var err error
	var matches [][]byte

	// read frames
	for {
		frame, err = flowd.ParseFrame(netin)
		if err != nil {
			if err == io.EOF {
				break
			}
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}

		// check for port close notification
		if frame.Type == "control" && frame.BodyType == "PortClose" {
			if !quiet {
				fmt.Fprintln(os.Stderr, "input port closed - exiting.")
			}
			// done
			break
		}

		// check for match
		matches = exp.FindSubmatch(frame.Body) //TODO [0] = match of entire expression, [1..] = submatches / match groups
		if matches == nil {
			// no match, forward unmodified
			if debug {
				fmt.Fprintln(os.Stderr, "no match, forwarding unmodified:", string(frame.Body))
			} else if !quiet {
				fmt.Fprintln(os.Stderr, "no match, forwarding unmodified")
			}
		} else {
			if debug {
				fmt.Fprintf(os.Stderr, "got matches, modifying: %v\n", matches)
			} else if !quiet {
				fmt.Fprintf(os.Stderr, "match found, setting %s to %s\n", field, string(matches[1]))
			}

			// modify frame
			if frame.Extensions == nil {
				if debug {
					fmt.Fprintln(os.Stderr, "frame extensions map is nil, initalizing")
				}
				frame.Extensions = map[string]string{}
			}
			frame.Extensions[field] = string(matches[1])
		}

		// send out modified frame
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
	fmt.Fprintln(os.Stderr, "IIP format: [flags] -field [header-field] [reg-exp]")
	fmt.Fprintln(os.Stderr, "expression is free argument may contain space, do not quote")
}
