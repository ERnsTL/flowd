package main

import (
	"bufio"
	"fmt"
	"io"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// check arguments
	//TODO
	// 1st argument = FIFO path
	inPipePath := os.Args[3+1] // [prog-name] -inport IN -inpath [fifo-path]
	// open FIFOs
	inPipe, err := os.OpenFile(inPipePath, os.O_RDONLY, os.ModeNamedPipe)
	if err != nil {
		panic(err)
	}
	// open connection to network and display output
	netin := bufio.NewReader(inPipe)
	errout := bufio.NewWriter(os.Stderr)
	defer errout.Flush()

	// main loop
	var frame *flowd.Frame
	//newline := []byte{'\n'}
	for {
		// read frame
		frame, err = flowd.Deserialize(netin)
		if err != nil {
			if err == io.EOF {
				fmt.Fprintln(os.Stderr, "EOF - exiting.")
				break
			}
			fmt.Fprintln(os.Stderr, "ERROR:", err, "- exiting.")
			break
		}

		// display frame body
		if frame.Body != nil {
			errout.Write(frame.Body)
			//os.Stdout.Write(frame.Body)
			// output newline only if needed
			if frame.Body[len(frame.Body)-1] != '\n' {
				errout.WriteByte('\n')
				//os.Stdout.Write(newline)
			}
		} else {
			errout.WriteString("<nil>")
			//os.Stdout.Write([]byte("<nil>"))
		}
		/*
			if netin.Buffered() == 0 {
				if err = errout.Flush(); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: flushing errout:", err)
				}
			}
		*/
	}
}
