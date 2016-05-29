package main

import (
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
	var bodyType, port, contentType string
	flag.StringVar(&bodyType, "bodytype", "", "data type of the frame body")
	flag.StringVar(&port, "port", "in", "port name for which this frame is destined")
	flag.StringVar(&contentType, "content-type", "text/plain", "MIME content type of the frame body")
	flag.BoolVar(&help, "h", false, "print usage information")
	flag.BoolVar(&debug, "debug", false, "print debug messages to stdout")
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Println("ERROR: unexpected free arguments")
		printUsage()
	}
	if help {
		printUsage()
	}

	// check arguments
	if bodyType == "" {
		fmt.Println("ERROR: missing body type")
		printUsage()
	}

	// check for stdin type
	if termutil.Isatty(os.Stdin.Fd()) {
		fmt.Println("ERROR: nothing piped on STDIN")
	} else {
		if debug {
			fmt.Println("ok, found something piped on STDIN")
		}

		// read body
		if debug {
			fmt.Println("reading body until EOF...")
		}
		var body bytes.Buffer
		io.Copy(&body, os.Stdin) // NOTE: copies from stdin to body buffer
		if debug {
			fmt.Println("total size:", body.Len())
		}

		// frame body
		if debug {
			fmt.Println("framing")
		}
		bodyBytes := body.Bytes() //TODO optimize
		frame := &flowd.Frame{
			Type:        "data",
			BodyType:    bodyType,
			Port:        port,
			ContentType: contentType,
			Body:        &bodyBytes,
			Extensions:  map[string]string{},
		}

		// output
		frame.Marshal(os.Stdout)
	}
}

func CheckError(err error) {
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
}

func printUsage() {
	fmt.Println("Usage:", os.Args[0], "-bodytype [data-type]", "-content-type [mime-type]", "-port [name]")
	flag.PrintDefaults()
	os.Exit(1)
}
