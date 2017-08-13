package main

import (
	"bufio"
	"bytes"
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/ERnsTL/flowd/libflowd"
	termutil "github.com/andrew-d/go-termutil"
)

func main() {
	var help, debug bool
	var bodyType, port string //, contentType string
	flag.StringVar(&bodyType, "bodytype", "", "data type of the frame body")
	flag.StringVar(&port, "port", "in", "port name for which this frame is destined")
	//flag.StringVar(&contentType, "content-type", "text/plain", "MIME content type of the frame body")
	flag.BoolVar(&help, "h", false, "print usage information")
	flag.BoolVar(&debug, "debug", false, "print debug messages to stderr")
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "ERROR: unexpected free arguments")
		printUsage()
	}
	if help {
		printUsage()
	}

	// check arguments
	if bodyType == "" {
		fmt.Fprintln(os.Stderr, "ERROR: missing body type")
		printUsage()
	}

	// check for stdin type
	if termutil.Isatty(os.Stdin.Fd()) {
		fmt.Fprintln(os.Stderr, "ERROR: nothing piped on STDIN")
	} else {
		if debug {
			fmt.Fprintln(os.Stderr, "ok, found something piped on STDIN")
		}

		// read body
		if debug {
			fmt.Fprintln(os.Stderr, "reading body until EOF...")
		}
		var body bytes.Buffer
		io.Copy(&body, os.Stdin) // NOTE: copies from stdin to body buffer
		if debug {
			fmt.Fprintln(os.Stderr, "total size:", body.Len())
		}

		// frame body
		if debug {
			fmt.Fprintln(os.Stderr, "framing")
		}
		bodyBytes := body.Bytes() //TODO optimize
		frame := &flowd.Frame{
			Type:     "data",
			BodyType: bodyType,
			Port:     port,
			//ContentType: contentType,
			Body:       bodyBytes,
			Extensions: map[string]string{},
		}
		if debug {
			errout := bufio.NewWriter(os.Stderr)
			frame.Marshal(errout)
			fmt.Fprintln(os.Stderr) // NOTE: just for nicer output
		}

		// output
		netout := bufio.NewWriter(os.Stdout)
		frame.Marshal(netout)
		if debug {
			fmt.Fprintln(os.Stderr, "done")
		}
	}
}

func checkError(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR:", err)
		os.Exit(2)
	}
}

func printUsage() {
	//fmt.Fprintln(os.Stderr, "Usage:", os.Args[0], "-bodytype [data-type]", "-content-type [mime-type]", "-port [name]")
	fmt.Fprintln(os.Stderr, "Usage:", os.Args[0], "-bodytype [data-type]", "-port [name]")
	flag.PrintDefaults()
	os.Exit(1)
}
