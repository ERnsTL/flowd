package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
)

type message struct {
	Name string
	Body string
	Time int64
}

func main() {
	// pre-declare to reduce GC allocations
	var inframe *flowd.Frame
	var err error
	outframe := flowd.Frame{
		Type:     "data",
		BodyType: "Message",
		Port:     "OUT",
		//ContentType: "application/json",
		Body: nil,
	}

	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	for {
		// read frame
		inframe, err = flowd.ParseFrame(netin)
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
		outframe.Marshal(netout)
	}
}
