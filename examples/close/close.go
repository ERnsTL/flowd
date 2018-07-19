// Closes its output port as a signal for the following component to exit.

package main

import (
	"fmt"
	"os"
)

func main() {
	// open connection to network
	portName := "OUT"
	portPath := os.Args[3+1] // [prog-name] -outport OUT -outpath [fifo-path]
	outPipe, err := os.OpenFile(portPath, os.O_WRONLY, os.ModeNamedPipe)
	if err != nil {
		fmt.Fprintf(os.Stderr, "ERROR: opening outport %s at path %s: %s\n", portName, portPath, err)
	}
	// create buffered writer
	//netout := bufio.NewWriter(outPipe)
	//defer netout.Flush()

	// close outport
	if err = outPipe.Close(); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: closing outport:", err)
	}
}
