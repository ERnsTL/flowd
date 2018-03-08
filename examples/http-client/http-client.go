package main

import (
	"bufio"
	"bytes"
	"fmt"
	"io/ioutil"
	"net/http"
	"os"
	"strconv"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
)

const timeout = 10 * time.Second //TODO make this configurable in IIP

//TODO convert to true FBP component useable together with tcp-client, unix-client etc.
// -> connection the Go HTTP client uses is a buffered writer into the FBP network -> tcp-client
// drawback currently: cannot set a tcp-client's remote host dynamically
func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()

	// commonly-used variables
	var frame *flowd.Frame
	var err error
	var found bool
	var method, url string
	var bodyr *bytes.Reader
	var req *http.Request
	httpClient := http.Client{
		Timeout: timeout,
	}
	var resp *http.Response

	// main loop
	for {
		// read frame
		frame, err = flowd.Deserialize(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: parsing frame:", err, "- exiting.")
			break
		}

		// check for closed ports
		if frame.Type == "control" && frame.BodyType == "PortClose" {
			fmt.Fprintln(os.Stderr, "received port close notification - exiting.")
			break
		}

		// check frame
		url, found = frame.Extensions["http-url"]
		if !found {
			fmt.Fprintln(os.Stderr, "WARNING: frame is missing http-url header - discarding.")
			continue
		}
		method, found = frame.Extensions["http-method"]
		if !found {
			fmt.Fprintln(os.Stderr, "WARNING: frame is missing http-method header - discarding.")
			continue
		}

		// create HTTP request
		bodyr = bytes.NewReader(frame.Body)
		req, err = http.NewRequest(method, url, bodyr)
		if err == nil {
			if method == "post" || method == "put" {
				// send frame body as request body
				//TODO anything to do for any of the other HTTP methods? ("get", "post", "options", "head", "put", "delete", "trace", "connect")
				req.Body = ioutil.NopCloser(bytes.NewBuffer(frame.Body))
			}
			for key, value := range frame.Extensions {
				// special fields
				switch key {
				case "http-method", "http-url":
					// do not put those into the HTTP request header
					continue
				default:
					// copy all others to HTTP response header
					req.Header.Add(key, value)
				}
			}
		}

		// send request
		if err == nil {
			resp, err = httpClient.Do(req)
		}

		// package response up into frame
		respFrame := &flowd.Frame{
			Port:       "OUT",
			Type:       "data",
			BodyType:   "HTTPResponse",
			Extensions: map[string]string{},
			Body:       nil,
		}
		if err != nil {
			respFrame.Extensions["http-error"] = err.Error()
		} else {
			respFrame.Extensions["http-status"] = strconv.Itoa(resp.StatusCode)
			if resp.StatusCode != 200 {
				respFrame.Extensions["http-error"] = resp.Status
			}
		}
		// read response body into frame body
		if err == nil {
			respFrame.Body, err = ioutil.ReadAll(resp.Body)
			if err != nil {
				fmt.Fprintln(os.Stderr, "WARNING: error reading HTTP response body:", err, "- discarding.")
				respFrame.Extensions["http-error"] = err.Error()
			}
			err = resp.Body.Close()
			if err != nil {
				respFrame.Extensions["http-error"] = err.Error()
			}
		}
		// copy response header
		if err == nil {
			for key, value := range resp.Header {
				respFrame.Extensions[key] = value[0] // first occurrence of that header field
			}
		}

		// send response to network
		if err := respFrame.Serialize(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling HTTP response frame downstream:", err, "- dropping.")
		}
		if netin.Buffered() == 0 {
			if err := netout.Flush(); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
			}
		}
		fmt.Fprintln(os.Stderr, "response forwarded")
	}
}
