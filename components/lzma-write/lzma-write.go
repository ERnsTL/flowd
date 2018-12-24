/*
Writer resp. encoder for LZMA2 from liblzma from the xzutils project.
Output is a raw = unframed compressed bytestream.

Requires C headers for liblzma:
  sudo apt install liblzma-dev
*/

package main

import (
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"

	xz "github.com/danielrh/go-xz"
)

func main() {
	// get configuration from arguments = Unix IIP
	var bridge bool
	var level int
	unixfbp.DefFlags()
	flag.BoolVar(&bridge, "bridge", false, "bridge mode, true = compress frames from/to FBP network, false = compress frame body")
	flag.IntVar(&level, "level", 9, "compression level (0=fast to 9=best)")
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "ERROR: unexpected free arguments")
		printUsage()
		flag.PrintDefaults()
		os.Exit(2)
	}
	if level < 0 || level > 9 {
		fmt.Fprintln(os.Stderr, "ERROR: compression level out of range [0;9]")
		printUsage()
		flag.PrintDefaults()
		os.Exit(2)
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

	// initialize xz/lzma2 writer
	xzWriter := xz.NewCompressionWriterPreset(netout, level)
	defer xzWriter.Close()

	// main loop
	if bridge {
		fmt.Fprintln(os.Stderr, "forwarding frames")
		// copy IN to compressing writer
		if _, err = io.Copy(&xzWriter, netin); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR on FBP->LZMA:", err)
			return
		}
		fmt.Fprintln(os.Stderr, "Input port closed - exiting.")

	} else {
		// handle FBP -> LZMA
		var frame *flowd.Frame
		for {
			frame, err = flowd.Deserialize(netin)
			if err != nil {
				if err == io.EOF {
					fmt.Fprintln(os.Stderr, "EOF - exiting.")
				} else {
					fmt.Fprintln(os.Stderr, "ERROR: parsing frame:", err, "- exiting.")
				}
				break
			}

			if unixfbp.Debug {
				fmt.Fprintln(os.Stderr, "received frame type", frame.Type, "data type", frame.BodyType, "for port", frame.Port, "with body:", string(frame.Body))
			} else if !unixfbp.Quiet {
				fmt.Fprintln(os.Stderr, "frame in with", len(frame.Body), "bytes body")
			}

			// write frame body out to LZMA writer
			if bytesWritten, err := xzWriter.Write(frame.Body); err != nil {
				// NOTE: library does not return io.EOF
				fmt.Fprintf(os.Stderr, "ERROR: writing to LZMA writer: %s - exiting.\n", err)
				break
			} else if bytesWritten < len(frame.Body) {
				// short write
				fmt.Fprintf(os.Stderr, "ERROR: short write to LZMA writer: only %d of %d bytes written - exiting.\n", bytesWritten, len(frame.Body))
				break
			} else {
				// success
				if !unixfbp.Quiet {
					fmt.Fprintf(os.Stderr, "wrote %d bytes to LZMA\n", bytesWritten)
				}
			}
		}
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Arguments: [-debug] [-quiet] [-bridge] [-level <0-9>]")
}
