package main

import (
	"bufio"
	"fmt"
	"os"
	"regexp"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
)

const maxFlushWait = 5 * time.Second // duration to wait until forcing STDOUT flush

func main() {
	// connect to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// flush netout after x seconds if there is buffered data
	go func() {
		for {
			time.Sleep(maxFlushWait)
			if netout.Buffered() > 0 {
				netout.Flush()
			}
		}
	}()
	// get configuration from IIP = initial information packet/frame
	modifications := map[string]string{}
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", netin); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		if len(iip) == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: received empty IIP, format is [field]=[value], eg. BodyType=Message Extension-Field=1")
			os.Exit(1)
		}
		// parse specifications
		//TODO add ability to set BodyType non-extension field
		//TODO add ability to set field to ""
		//TODO add ability to delete header field
		re := regexp.MustCompile("(.+)=(.*)")
		args := re.FindAllStringSubmatch(iip, -1)
		if args == nil {
			fmt.Fprintln(os.Stderr, "ERROR: no modification specification(s) found")
			fmt.Fprintln(os.Stderr, "IIP format: [field]=[value]...")
			os.Exit(1)
		}
		for _, arg := range args {
			field := arg[1]
			value := arg[2]
			//TODO if debug fmt.Fprintf(os.Stderr, "got modification %s to %s\n", field, value)
			modifications[field] = value
		}
	}
	fmt.Fprintf(os.Stderr, "got %d modification specification(s)\n", len(modifications))

	// prepare variables
	var frame *flowd.Frame
	var err error

	// main loop
	for {
		// read frame
		frame, err = flowd.ParseFrame(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// check for port closed
		if frame.Type == "control" && frame.BodyType == "PortClose" && frame.Port == "IN" {
			fmt.Fprintln(os.Stderr, "input port closed - exiting.")
			break
		}

		// apply modification(s)
		//TODO make this configurable using "debug" parameter in IIP
		fmt.Fprintln(os.Stderr, "got frame, modifying...")
		if frame.Extensions == nil {
			frame.Extensions = map[string]string{}
		}
		for field, value := range modifications {
			fmt.Fprintf(os.Stderr, "\tsetting %s to %s\n", field, value)
			frame.Extensions[field] = value
		}
		fmt.Fprintln(os.Stderr, "modified. forwarding now.")

		// send it to given output ports
		frame.Port = "OUT"
		if err = frame.Marshal(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
		}
	}
}
