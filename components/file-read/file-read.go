package main

import (
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
)

const bufSize = 65536

func main() {
	// flag variables
	var filePaths []string
	var brackets bool
	// get configuration from flags = Unix IIP (which can be generated out of another FIFO or similar)
	unixfbp.DefFlags()
	flag.BoolVar(&brackets, "brackets", false, "enclose each file in brackets")
	flag.Parse()
	if flag.NArg() == 0 {
		fmt.Fprintln(os.Stderr, "ERROR: missing filepath(s) to read")
		printUsage()
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	filePaths = flag.Args()
	if !unixfbp.Quiet {
		fmt.Fprintf(os.Stderr, "starting up, got %d filepaths\n", len(filePaths))
	}
	// open network connections
	netout, _, err := unixfbp.OpenOutPort("OUT")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	defer netout.Flush()
	//defer netoutFile.Close()

	// prepare variables
	outframe := &flowd.Frame{
		Type:     "data",
		BodyType: "FileChunk",
		//Port:     "OUT",
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
			obracket.Serialize(netout)
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
			if unixfbp.Debug {
				fmt.Fprintf(os.Stderr, "file %d: read %d bytes\n", i+1, n)
			}

			// save as body
			outframe.Body = buf[:n]

			// send it to output port
			if err = outframe.Serialize(netout); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err)
				break
			}
		}

		// close file
		err = file.Close()
		if err != nil {
			fmt.Fprintf(os.Stderr, "ERROR: closing file '%s': %v\n", filePaths[i], err)
		}

		// send close bracket
		if brackets {
			cbracket.Serialize(netout)
		}

		// report
		if !unixfbp.Quiet {
			fmt.Fprintf(os.Stderr, "completed file %d of %d\n", i+1, len(filePaths))
		}
	}

	// report
	if unixfbp.Debug {
		fmt.Fprintln(os.Stderr, "completed all")
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Arguments: [flags] [file-path]...")
}
