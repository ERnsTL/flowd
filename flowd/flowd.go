package main

import (
	"fmt"
	"io/ioutil"
	"os"

	termutil "github.com/andrew-d/go-termutil"
	"github.com/oleksandr/fbp"
)

func main() {
	if termutil.Isatty(os.Stdin.Fd()) {
		fmt.Println("ERROR: nothing piped on STDIN, expected network definition")
	} else {
		fmt.Println("ok, found something piped on STDIN")

		// read network definition
		nwBytes, err := ioutil.ReadAll(os.Stdin)
		if err != nil {
			fmt.Println(err.Error())
			os.Exit(1)
		}

		// parse and validate network
		nw := &fbp.Fbp{Buffer: (string)(nwBytes)}
		fmt.Println("init")
		nw.Init()
		fmt.Println("parse")
		if err := nw.Parse(); err != nil {
			fmt.Println(err.Error())
			os.Exit(1)
		}
		fmt.Println("execute")
		nw.Execute()
		fmt.Println("validate")
		if err := nw.Validate(); err != nil {
			fmt.Println(err.Error())
			os.Exit(1)
		}
		fmt.Println("network definition OK")

		// display all data
		fmt.Println("subgraph name:", nw.Subgraph)
		fmt.Println("processes:")
		for _, p := range nw.Processes {
			fmt.Println(" ", p.String())
		}
		fmt.Println("connections:")
		for _, c := range nw.Connections {
			fmt.Println(" ", c.String())
		}
		fmt.Println("input ports:")
		for _, i := range nw.Inports {
			fmt.Println(" ", i.String())
		}
		fmt.Println("output ports:")
		for _, o := range nw.Outports {
			fmt.Println(" ", o.String())
		}
	}
}
