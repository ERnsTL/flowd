package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
)

const blockSize = 65536

func main() {
	// open connection to network
	bufr := bufio.NewReader(os.Stdin)
	// flag variables
	var filePaths []string
	var debug, quiet, brackets bool
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", bufr); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		// parse IIP
		flags := flag.NewFlagSet("cmd", flag.ContinueOnError)
		flags.BoolVar(&brackets, "brackets", false, "enclose each file in brackets")
		flags.BoolVar(&debug, "debug", false, "give detailed event output")
		flags.BoolVar(&quiet, "quiet", false, "no informational output except errors")
		if err := flags.Parse(strings.Split(iip, " ")); err != nil {
			os.Exit(2)
		}
		if flags.NArg() == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: missing filepath(s) to read")
			printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
		filePaths = flags.Args()
	}
	fmt.Fprintf(os.Stderr, "starting up, got %d filepaths\n", len(filePaths))

	// prepare variables
	outframe := &flowd.Frame{ //TODO why is this pointer to Frame?
		Type:     "data",
		BodyType: "FileChunk",
		Port:     "OUT",
	}
	var obracket, cbracket flowd.Frame
	if brackets {
		obracket = flowd.BracketOpen("OUT")
		cbracket = flowd.BracketClose("OUT")
	}
	buf := make([]byte, blockSize)

	// main work loops
	// TODO does the order really matter?
	for i := 0; i < len(filePaths); i++ {
		// open file
		file, err := os.Open(filePaths[i])
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			continue
		}

		// send open bracket
		//TODO send filename also?
		if brackets {
			obracket.Marshal(os.Stdout)
		}

		// read it in chunks until end, forward chunks
		for {
			// read chunk
			n, err := file.Read(buf)
			if err != nil {
				// just EOF or did any real error occur?
				if err != io.EOF {
					fmt.Fprintln(os.Stderr, "ERROR: reading file:", err)
				}
				// done with this file
				break
			}
			if debug {
				fmt.Fprintf(os.Stderr, "file %d: read %d bytes\n", i+1, n)
			}

			// save as body
			outframe.Body = buf[:n]

			// send it to given output ports
			if err := outframe.Marshal(os.Stdout); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err)
			}
		}

		// send close bracket
		if brackets {
			cbracket.Marshal(os.Stdout)
		}

		// report
		if !quiet {
			fmt.Fprintf(os.Stderr, "completed file %d of %d\n", i+1, len(filePaths))
		}
	}

	// report
	if debug {
		fmt.Fprintln(os.Stderr, "completed all")
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Usage: "+os.Args[0]+" [flags] [file-path]...")
}
