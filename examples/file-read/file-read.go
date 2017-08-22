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

const bufSize = 65536

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// flag variables
	var filePaths []string
	var debug, quiet, brackets bool
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", netin); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		// parse IIP
		flags := flag.NewFlagSet("file-read", flag.ContinueOnError)
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
	if !quiet {
		fmt.Fprintf(os.Stderr, "starting up, got %d filepaths\n", len(filePaths))
	}

	// prepare variables
	outframe := &flowd.Frame{ //TODO why is this pointer to Frame?
		Type:     "data",
		BodyType: "FileChunk",
		Port:     "OUT",
	}
	var obracket, cbracket flowd.Frame
	var n int
	if brackets {
		obracket = flowd.BracketOpen("OUT")
		cbracket = flowd.BracketClose("OUT")
	}
	buf := make([]byte, bufSize)

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
			obracket.Marshal(netout)
		}

		// read it in chunks until end, forward chunks
		for {
			// read chunk
			n, err = file.Read(buf)
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

			// send it to output port
			if err = outframe.Marshal(netout); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err)
			}
		}

		// close file
		err = file.Close()
		if err != nil {
			fmt.Fprintf(os.Stderr, "ERROR: closing file '%s': %v\n", filePaths[i], err)
		}

		// send close bracket
		if brackets {
			cbracket.Marshal(netout)
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
	fmt.Fprintln(os.Stderr, "IIP format: [flags] [file-path]...")
}
