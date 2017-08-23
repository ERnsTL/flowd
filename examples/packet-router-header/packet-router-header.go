package main

import (
	"bufio"
	"flag"
	"fmt"
	"net/textproto"
	"os"
	"strings"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
)

const maxFlushWait = 5 * time.Second // duration to wait until forcing STDERR flush

var (
	rules     = map[string]string{} // keeps value -> target-output-port mappings
	rulesTemp string                // state variable for flag parsing, keeps -equals value
)

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// flush netout after x seconds if there is buffered data
	go func() {
		for {
			time.Sleep(maxFlushWait)
			if netout.Buffered() > 0 {
				netout.Flush()
			}
		}
	}()
	// flag variables
	var debug, quiet bool
	var field, present, missing, nomatchPort string
	var equals equalsFlag
	var to toFlag
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", netin); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		// parse IIP
		flags := flag.NewFlagSet("packet-router-header", flag.ContinueOnError)
		flags.BoolVar(&debug, "debug", false, "give detailed event output")
		flags.BoolVar(&quiet, "quiet", false, "no informational output except errors")
		flags.StringVar(&field, "field", "", "header field to inspect")
		flags.StringVar(&present, "present", "", "output port for packets with header field present")
		flags.StringVar(&missing, "missing", "NOMATCH", "output port for packets with header field missing")
		flags.StringVar(&nomatchPort, "nomatch", "NOMATCH", "port for unmatched packets")
		flags.Var(&equals, "equals", "matching value in header field")
		flags.Var(&to, "to", "destination for matching value in header field")
		if err := flags.Parse(strings.Split(iip, " ")); err != nil {
			os.Exit(2)
		}
		// check flags
		if rulesTemp != "" {
			fmt.Fprintln(os.Stderr, "ERROR: -equals without following -to, but both required")
			printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
		if (present != "" && len(rules) != 0) || (present == "" && len(rules) == 0) {
			fmt.Fprintln(os.Stderr, "ERROR: either -present or -equals expected")
			printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
		if field == "" {
			fmt.Fprintln(os.Stderr, "ERROR: -field missing")
			printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
		if flags.NArg() != 0 {
			fmt.Fprintln(os.Stderr, "ERROR: unexpected free argument encountered")
			printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
	}
	if !quiet {
		fmt.Fprintln(os.Stderr, "starting up")
	}

	// convert field name to canonical MIME name
	field = textproto.CanonicalMIMEHeaderKey(field)

	// generate frame matchers
	//TODO possible optimization regarding *string return value
	var ruleFuncs []ruleMatcher
	// header value missing
	if missing != "" {
		ruleFuncs = append(ruleFuncs, func(value *string) *string {
			if value == nil {
				return &missing
			}
			// no match
			return nil
		})
	}
	// header value present
	if present != "" {
		ruleFuncs = append(ruleFuncs, func(value *string) *string {
			if value != nil {
				return &present
			}
			// no match
			return nil
		})
	}
	// regular equality rules
	if len(rules) > 0 {
		if debug {
			fmt.Fprintln(os.Stderr, "routing table:")
		}
		for matchValue, targetPort := range rules {
			// make copies so that func gets local copy, otherwise all rules would be the same
			matchValueCopy := matchValue
			targetPortCopy := targetPort
			if debug {
				fmt.Fprintf(os.Stderr, "\tif %s equals %s, forward to %s\n", field, matchValue, targetPort)
			}
			ruleFuncs = append(ruleFuncs, func(value *string) *string {
				if value == nil {
					// not responsible
					return nil
				}
				if *value == matchValueCopy {
					return &targetPortCopy
				}
				// no match
				return nil
			})
		}
		if debug {
			fmt.Fprintf(os.Stderr, "\tif %s missing, forward to %s\n", field, nomatchPort)
		}
	}
	// default catch-all rule
	ruleFuncs = append(ruleFuncs, func(value *string) *string {
		return &nomatchPort
	})
	// empty rules map
	rules = nil

	// header field getter
	var fieldGetter fieldGetter
	switch field {
	case "Type":
		fieldGetter = func(frame *flowd.Frame) *string {
			return &frame.Type
		}
	case "BodyType":
		fieldGetter = func(frame *flowd.Frame) *string {
			return &frame.BodyType
		}
	default:
		fieldGetter = func(frame *flowd.Frame) *string {
			if frame.Extensions == nil {
				// nothing there
				return nil
			}
			if value, exists := frame.Extensions[field]; exists {
				return &value
			}
			// field missing
			return nil
		}
	}

	// prepare variables
	var frame *flowd.Frame
	var err error
	var fieldValue *string

	// main work loop
	//TODO make many outputs configurable by -debug
nextframe:
	for {
		// read frame
		frame, err = flowd.ParseFrame(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// check for closed input port
		if frame.Type == "control" && frame.BodyType == "PortClose" && frame.Port == "IN" {
			// shut down operations
			fmt.Fprintln(os.Stderr, "input port closed - exiting.")
			break
		}

		// get field value
		/*
			if frame.Extensions != nil {
				if value, found := frame.Extensions["Myfield"]; found {
					fmt.Fprintf(os.Stderr, "in frame directly: field has value %s\n", value)
				} else {
					fmt.Fprintln(os.Stderr, "in frame directly: field not found")
				}
			} else {
				fmt.Fprintln(os.Stderr, "in frame directly: field not found")
			}
		*/
		fieldValue = fieldGetter(frame)
		if debug {
			if fieldValue != nil {
				fmt.Fprintf(os.Stderr, "field %s has value %s\n", field, *fieldValue)
			} else {
				fmt.Fprintf(os.Stderr, "field %s has value %v\n", field, fieldValue)
			}
		}

		// check which rule applies
		for _, ruleFunc := range ruleFuncs {
			if targetPort := ruleFunc(fieldValue); targetPort != nil {
				// rule applies, forward frame to returned port
				if debug {
					fmt.Fprintf(os.Stderr, "forwarding to port %s\n", *targetPort)
				}
				frame.Port = *targetPort
				if err := frame.Marshal(netout); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err)
				}
				// done with this frame
				continue nextframe
			}
		}

		// no rule matched, not even final catch-all rule
		fmt.Fprintln(os.Stderr, "ERROR: no rule matched, should never be reached - exiting.")
		os.Exit(1)
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "IIP format: [-field] [-missing] [-present|-equals] [-equals]...")
}

// flag acceptors of -equals [value] -to [output-port] couples
// NOTE: need two separate in order to know that value came from which flag
type equalsFlag struct{}

func (f *equalsFlag) String() string {
	// return default value
	return ""
}

func (f *equalsFlag) Set(value string) error {
	if rulesTemp == "" {
		// -equals flag given, fill it
		rulesTemp = value
		return nil
	}
	// -equals flag was previously given, but -to expected, not another -equals
	return fmt.Errorf("-equals followed by -equals, expected -to")
}

type toFlag struct{}

func (f *toFlag) String() string {
	// return default value
	return ""
}

func (f *toFlag) Set(value string) error {
	if rulesTemp == "" {
		// no value from previous -equals flag
		return fmt.Errorf("-to without preceding -equals")
	}
	// all data given for a rule, save it
	rules[rulesTemp] = value
	// reset for next -equals -to duet
	rulesTemp = ""
	return nil
}

// frame field retrieval and rule matcher function definitions
type ruleMatcher func(*string) *string
type fieldGetter func(*flowd.Frame) *string
