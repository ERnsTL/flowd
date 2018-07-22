package main

import (
	"bufio"
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
	var framed bool
	var filePath string
	// get configuration from flags = Unix IIP
	flag.BoolVar(&framed, "framed", false, "expect framed data on pipe or raw data")
	unixfbp.DefFlags()

	flag.Parse()
	if flag.NArg() != 1 {
		fmt.Fprintln(os.Stderr, "ERROR: expecting one filepath as free argument")
		printUsage()
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	filePath = flag.Args()[0]

	// open network connections
	netin, _, err := unixfbp.OpenInPort("IN")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}

	// open file
	file, err := os.Create(filePath) // NOTE: truncates to file size 0
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}
	fileWriter := bufio.NewWriterSize(file, bufSize)

	if framed {
		// prepare variables
		var frame *flowd.Frame

		// read it in chunks until end, write to file
		for {
			frame, err = flowd.Deserialize(netin)
			if err != nil {
				if err == io.EOF {
					// end of file is inevitable
					break
				} else {
					fmt.Fprintln(os.Stderr, "ERROR parsing frame from FBP network:", err, "- exiting.")
				}
				os.Exit(2)
			}

			// write chunk
			if bytesWritten, err := fileWriter.Write(frame.Body); err != nil {
				fmt.Fprintf(os.Stderr, "ERROR: writing to file: %s - exiting.\n", err)
				os.Exit(2)
			} else if bytesWritten < len(frame.Body) {
				// short write
				fmt.Fprintf(os.Stderr, "ERROR: short write to file: only %d of %d bytes written - exiting.\n", bytesWritten, len(frame.Body))
				os.Exit(2)
			} else {
				// success
				if !unixfbp.Quiet {
					fmt.Fprintf(os.Stderr, "wrote %d bytes to file\n", bytesWritten)
				}
			}
		}
	} else {
		// write unframed resp. raw data to file
		io.Copy(fileWriter, netin)
	}

	// flush writer
	if err = fileWriter.Flush(); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: flushing writer:", err)
		return
	}

	// close file
	if err = file.Close(); err != nil {
		fmt.Fprintf(os.Stderr, "ERROR: closing file '%s': %v\n", filePath, err)
		return
	}

	// report
	if unixfbp.Debug {
		fmt.Fprintln(os.Stderr, "exiting")
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Arguments: [flags] [file-path]")
}
