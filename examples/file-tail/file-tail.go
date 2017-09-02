package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/hpcloud/tail"
)

//const maxFlushWait = 100 * time.Millisecond // flush any buffered outgoing frames after at most this duration

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// flush netout after x seconds if there is buffered data
	// NOTE: bufio.Writer.Write() flushes on its own if buffer is full
	/*
		go func() {
			for {
				time.Sleep(maxFlushWait)
				// NOTE: Flush() checks on its own if data buffered
				netout.Flush()
			}
		}()
	*/
	// flag variables
	var filePath string
	var debug, quiet bool
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", netin); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		// parse IIP
		flags := flag.NewFlagSet("file-tail", flag.ContinueOnError)
		flags.BoolVar(&debug, "debug", false, "give detailed event output")
		flags.BoolVar(&quiet, "quiet", false, "no informational output except errors")
		if err := flags.Parse(strings.Split(iip, " ")); err != nil {
			os.Exit(2)
		}
		if flags.NArg() == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: missing filepath to read")
			printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		} else if flags.NArg() > 1 {
			fmt.Fprintln(os.Stderr, "ERROR: more than one filepath unimplemented")
			printUsage()
			flags.PrintDefaults() // prints to STDERR
			os.Exit(2)
		}
		filePath = flags.Args()[0]
	}
	if !debug {
		fmt.Fprintln(os.Stderr, "tailing")
	} else {
		fmt.Fprintf(os.Stderr, "now tailing, filepath is %s\n", filePath)
	}

	// prepare variables
	outframe := &flowd.Frame{ //TODO why is this pointer to Frame?
		Type:     "data",
		BodyType: "FileLine",
		Port:     "OUT",
	}

	// start tailing
	t, err := tail.TailFile(filePath, tail.Config{Location: &tail.SeekInfo{Offset: 0, Whence: os.SEEK_END}, ReOpen: true, Follow: true, Logger: tail.DiscardingLogger})
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: in call to TailFile():", err.Error())
		os.Exit(1)
	}

	// main work loop
	for line := range t.Lines {
		if !quiet {
			fmt.Fprintf(os.Stderr, "received line (%d bytes)\n", len([]byte(line.Text)))
		} else if debug {
			fmt.Fprintf(os.Stderr, "received line (%d bytes): %s\n", len([]byte(line.Text)), line.Text)
		}

		// save as body
		outframe.Body = []byte(line.Text)

		// send it to given output ports
		if err = outframe.Marshal(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err)
		}
		if err = netout.Flush(); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
		}
	}

	//TODO add error handling on line.Err

	// report
	if debug {
		fmt.Fprintln(os.Stderr, "all done")
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "IIP format: [flags] [file-path]")
}
