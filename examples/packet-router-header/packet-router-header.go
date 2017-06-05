package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
)

var (
	rules     map[string]string // keeps value -> target-output-port mappings
	rulesTemp string            // state variable for flag parsing, keeps -equals value
)

func main() {
	// open connection to network
	bufr := bufio.NewReader(os.Stdin)
	// flag variables
	var debug, quiet bool
	var field, present, missing, nomatchPort string
	var equals equalsFlag
	var to equalsFlag
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", bufr); err != nil {
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
		for matchValue, targetPort := range rules {
			ruleFuncs = append(ruleFuncs, func(value *string) *string {
				if *value == matchValue {
					return &targetPort
				}
				// no match
				return nil
			})
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

	// main work loop
nextframe:
	for {
		// read frame
		frame, err = flowd.ParseFrame(bufr)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// check for closed input port
		if frame.Type == "control" && frame.BodyType == "PortClose" {
			// shut down operations
			fmt.Fprintln(os.Stderr, "input port closed - exiting.")
			break
		}

		// get field value
		fieldValue := fieldGetter(frame)

		// check which rule applies
		for _, ruleFunc := range ruleFuncs {
			if targetPort := ruleFunc(fieldValue); targetPort != nil {
				// rule applies, forward frame to returned port
				frame.Port = *targetPort
				if err := frame.Marshal(os.Stdout); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err)
				}
				// done with this frame
				break nextframe
			}
		}

		// no rule matched, not even final catch-all rule
		fmt.Fprintln(os.Stderr, "ERROR: no rule matched, should never be reached - exiting.")
		os.Exit(1)
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Usage: "+os.Args[0]+" [-field] [-missing] [-present|-equals] [-equals]...")
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
