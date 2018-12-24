/*
Reader resp. decoder for LZMA2 from liblzma from the xzutils project.
Input is a raw = unframed compressed bytestream; output is a stream of frames.

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

const bufSize = 65536

func main() {
	// get configuration from arguments = Unix IIP
	var bridge bool
	unixfbp.DefFlags()
	flag.BoolVar(&bridge, "bridge", false, "bridge mode, true = decompress frames from/to FBP network, false = decompress into frame body")
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "ERROR: unexpected free arguments")
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
	xzReader := xz.NewDecompressionReader(netin)

	// main loop
	if bridge {
		fmt.Fprintln(os.Stderr, "forwarding frames")
		// copy decompressing reader into OUT
		if _, err = io.Copy(netout, &xzReader); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR on FBP->LZMA:", err)
			return
		}
		fmt.Fprintln(os.Stderr, "Input port closed - exiting.")

	} else {
		// handle LZMA -> FBP
		outframe := &flowd.Frame{
			Type:     "data",
			BodyType: "DataChunk",
		}
		buf := make([]byte, bufSize)
		var n int
		for {
			// read chunk
			n, err = xzReader.Read(buf)
			if err != nil {
				// just EOF or did any real error occur?
				if err != io.EOF {
					fmt.Fprintln(os.Stderr, "ERROR: reading netin:", err)
					break
				}
				// done with the input (EOF)
				if n == 0 {
					fmt.Fprintln(os.Stderr, "input reached EOF")
					break
				}
			}
			if unixfbp.Debug {
				fmt.Fprintf(os.Stderr, "got %d bytes\n", n)
			}

			// save as body
			outframe.Body = buf[:n]

			// send it to output port
			if err = outframe.Serialize(netout); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: serializing frame:", err)
				break
			}

			// flush if nothing buffered on input port
			if netin.Buffered() == 0 {
				if err = netout.Flush(); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
				}
			}
		}
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Arguments: [-debug] [-quiet] [-bridge]")
}
