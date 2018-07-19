package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/ERnsTL/UnixFBP/libunixfbp"
	"github.com/ERnsTL/flowd/libflowd"
	"github.com/hpcloud/tail"
)

func main() {
	// flag variables
	var filePath string
	// get configuration from flags = Unix IIP
	unixfbp.DefFlags()
	flag.Parse()
	if flag.NArg() == 0 {
		fmt.Fprintln(os.Stderr, "ERROR: missing filepath to read")
		printUsage()
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	} else if flag.NArg() > 1 {
		fmt.Fprintln(os.Stderr, "ERROR: more than one filepath currently unimplemented")
		printUsage()
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	// parse free arguments
	filePath = flag.Arg(0)
	// open network connections
	netout, _, err := unixfbp.OpenOutPort("OUT")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	defer netout.Flush()

	// flag variables
	if !unixfbp.Quiet {
		fmt.Fprintln(os.Stderr, "tailing")
	} else if unixfbp.Debug {
		fmt.Fprintf(os.Stderr, "now tailing, filepath is %s\n", filePath)
	}

	// prepare variables
	outframe := &flowd.Frame{ //TODO why is this pointer to Frame?
		Type:     "data",
		BodyType: "FileLine",
		//Port:     "OUT",
	}

	// start tailing
	t, err := tail.TailFile(filePath, tail.Config{Location: &tail.SeekInfo{Offset: 0, Whence: os.SEEK_END}, ReOpen: true, Follow: true, Logger: tail.DiscardingLogger})
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: in call to TailFile():", err.Error())
		os.Exit(1)
	}

	// main work loop
	for line := range t.Lines {
		if !unixfbp.Quiet {
			fmt.Fprintf(os.Stderr, "received line (%d bytes)\n", len([]byte(line.Text)))
		} else if unixfbp.Debug {
			fmt.Fprintf(os.Stderr, "received line (%d bytes): %s\n", len([]byte(line.Text)), line.Text)
		}

		// save as body
		outframe.Body = []byte(line.Text)

		// send it to given output ports
		if err = outframe.Serialize(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: serializing frame:", err)
		}
	}

	//TODO add error handling on line.Err

	// report
	if unixfbp.Debug {
		fmt.Fprintln(os.Stderr, "all done")
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Arguments: [flags] [file-path]")
}
