package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"

	"github.com/ERnsTL/UnixFBP/libunixfbp"
	"github.com/ERnsTL/flowd/libflowd"
)

type message struct {
	Name string
	Body string
	Time int64
}

func main() {
	// get configuration from flags
	unixfbp.DefFlags()
	flag.Parse()

	// connect to FBP network
	var err error
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

	// pre-declare to reduce GC allocations
	var inframe *flowd.Frame
	outframe := flowd.Frame{
		Type:     "data",
		BodyType: "Message",
		//Port:     "OUT",
		//ContentType: "application/json",
		Body: nil,
	}
	// main loop
	for {
		// read frame
		inframe, err = flowd.Deserialize(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// parse JSON body
		var records []map[string]interface{} //TODO is not actually using message type
		if err := json.Unmarshal(inframe.Body, &records); err != nil {
			fmt.Fprintln(os.Stderr, err)
			return
		}

		// process
		for record := range records {
			for field := range records[record] {
				if field != "Name" {
					delete(records[record], field)
				}
			}
		}

		// encode result
		body, err := json.Marshal(&records)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			return
		}
		// frame result
		outframe.Body = body
		outframe.Extensions = inframe.Extensions

		// send it
		outframe.Serialize(netout)

		// flush
		if netin.Buffered() == 0 {
			if err = netout.Flush(); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
			}
		}
	}
}
