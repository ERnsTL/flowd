package main

import (
	"encoding/json"
	"fmt"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
)

type Message struct {
	Name string
	Body string
	Time int64
}

func main() {
	// pre-declare to reduce GC allocations
	var inframe *flowd.Frame
	var err error
	outframe := flowd.Frame{
		Type:        "data",
		BodyType:    "Message",
		Port:        "out",
		ContentType: "application/json",
		Body:        nil,
	}

	for {
		// read frame
		inframe, err = flowd.ParseFrame(os.Stdin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// parse JSON body
		var records []map[string]interface{}
		if err := json.Unmarshal(*inframe.Body, &records); err != nil {
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
		if body, err := json.Marshal(&records); err != nil {
			fmt.Fprintln(os.Stderr, err)
			return
		} else {
			// frame result
			outframe.Body = &body

			// send it
			outframe.Marshal(os.Stdout)
		}
	}
}
