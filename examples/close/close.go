package main

import (
	"bufio"
	"fmt"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
)

/*
Closes its output port as a signal for the following component to exit.
TODO flowd: Send port close notification if all IIPs have been send and no input is connected.
	-> making this component useless.
TODO .fbp parser: Cannot currently have just a single component -> need this close component.
*/

func main() {
	// open connection to network
	//netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()

	// generate port close control frame
	frame := flowd.PortClose("OUT")

	// send it to output port
	if err := frame.Marshal(netout); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
	}
}
