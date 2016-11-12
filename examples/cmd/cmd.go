package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
)

type OperatingMode int

const (
	One OperatingMode = iota
	Each
)

func (op *OperatingMode) String() string {
	op2 := *op //FIXME how to do this correctly? type switch?
	switch op2 {
	case One:
		return "one call handling all input IPs"
	case Each:
		return "one call for each input IP"
	default:
		if op == nil {
			return "nil"
		} else {
			return "ERROR value out of range"
		}
	}
}

// implement flag.Value interface
func (op *OperatingMode) Set(value string) error {
	switch value {
	case "one":
		*op = One
	case "each":
		*op = Each
	default:
		return fmt.Errorf("set of allowable values for operating mode is {one, each}")
	}
	return nil
}

func main() {
	// open connection to network
	bufr := bufio.NewReader(os.Stdin)
	// flag variables
	var operatingMode OperatingMode
	var outframing bool
	var retry bool
	var cmdargs []string
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", bufr); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		// parse IIP
		//var debug, quiet bool
		flags := flag.NewFlagSet("cmd", flag.ContinueOnError)
		flags.Var(&operatingMode, "mode", "operating mode: one (command instance handling all IPs) or each (IP handled by new instance)")
		flags.BoolVar(&outframing, "outframing", false, "perform frame encoding on command output, false means already framed")
		flags.BoolVar(&retry, "retry", false, "retry/restart command on non-zero return code")
		//flags.BoolVar(&debug, "debug", false, "give detailed event output")
		//flags.BoolVar(&quiet, "quiet", false, "no informational output except errors")
		if err := flags.Parse(strings.Split(iip, " ")); err != nil {
			os.Exit(2)
		}
		if flags.NArg() == 0 {
			fmt.Println("ERROR: missing command to run")
			printUsage()
			flags.PrintDefaults()
			os.Exit(2)
		}
		cmdargs = flags.Args()
	}
	//fmt.Fprintf(os.Stderr, "starting up in operating mode: %s, output framing: %t, retry: %t \n", operatingMode.String(), outframing, retry)
	fmt.Fprintln(os.Stderr, "starting up, command is", strings.Join(cmdargs, " "))

	var frame *flowd.Frame //TODO why is this pointer to Frame?
	var err error

	for {
		// read frame
		frame, err = flowd.ParseFrame(bufr)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		fmt.Fprintln(os.Stderr, "got packet:", string(frame.Body))

		//TODO implement actually running subprocess
		//TODO implement timeout on subprocess etc.
		//TODO implement retry/restart

		// send it to given output ports
		/*
			for _, outPort := range outPorts {
				frame.Port = outPort
				if err := frame.Marshal(os.Stdout); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
				}
			}
		*/
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Usage: cmd [flags] [cmdpath] [args]...")
	os.Exit(2)
}
