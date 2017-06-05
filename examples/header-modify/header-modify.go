package main

import (
	"bufio"
	"fmt"
	"os"
	"regexp"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// get configuration from IIP = initial information packet/frame
	modifications := map[string]string{}
	bufr := bufio.NewReader(os.Stdin)
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", bufr); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		if len(iip) == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: received empty IIP, format is [field]=[value], eg. BodyType=Message Extension-Field=1")
			os.Exit(1)
		}
		// parse specifications
		//TODO add ability to set BodyType non-extension field
		re := regexp.MustCompile("(.+)=(.*)")
		args := re.FindAllStringSubmatch(iip, -1)
		if args == nil {
			fmt.Fprintln(os.Stderr, "ERROR: no modification specification(s) found")
			os.Exit(1)
		}
		for _, arg := range args {
			field := arg[1]
			value := arg[2]
			//fmt.Fprintf(os.Stderr, "got modification %s to %s\n", field, value)
			modifications[field] = value
		}
	}
	fmt.Fprintf(os.Stderr, "got %d modification specification\n", len(modifications))

	// prepare variables
	var frame *flowd.Frame
	var err error

	// main loop
	for {
		// read frame
		frame, err = flowd.ParseFrame(bufr)
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
		if err = frame.Marshal(os.Stdout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
		}
	}
}
