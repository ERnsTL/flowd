package main

import (
	"bufio"
	"fmt"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// get configuration from IIP = initial information packet/frame
	var modFunc ModFunc
	bufr := bufio.NewReader(os.Stdin)
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", bufr); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		if len(iip) == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: received empty IIP, format is [field]=[value], eg. BodyType=Message or Extension-Field=1")
			os.Exit(1)
		} else if modFunc, err = parseModSpec(iip); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: malformed IIP, format is [field]=[value], eg. BodyType=Message or Extension-Field=1")
			os.Exit(1)
		}
	}
	fmt.Fprintln(os.Stderr, "got modification specification")

	var frame *flowd.Frame
	var err error

	for {
		// read frame
		frame, err = flowd.ParseFrame(bufr)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// apply modification
		//TODO make this configurable using "debug" parameter in IIP
		fmt.Fprintln(os.Stderr, "got frame, modifying...")
		modFunc(frame)
		fmt.Fprintln(os.Stderr, "modified. forwarding now.")

		// send it to given output ports
		frame.Port = "OUT"
		if err := frame.Marshal(os.Stdout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
		}
	}
}

type ModFunc func(*flowd.Frame)

func parseModSpec(spec string) (ModFunc, error) {
	//TODO
	return func(frame *flowd.Frame) {
		if frame.Extensions == nil {
			frame.Extensions = map[string]string{}
		}
		frame.Extensions["Blob-Name"] = "sensor1.temperature.value"
		fmt.Fprintln(os.Stderr, "ok, Blob-Name is now", frame.Extensions["Blob-Name"])
	}, nil
}
