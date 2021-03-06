/*
a simple HTTP server

ports:
- IN for connections from a network transport
- OUT for sending out the HTTP requests to a handler inside the FBP network
- RESP (inport) to receive responses from the handler(s)
- RESP (outport) to send HTTP-packaged responses back over the network transport
*/

package main

import (
	"bufio"
	"bytes"
	"flag"
	"fmt"
	"io"
	"io/ioutil"
	"net/http"
	"os"
	"strconv"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
)

const timeout = 10 * time.Second

// Client is used for data exchange between main loop and connection handler
type Client struct {
	pipew *io.PipeWriter // write HTTP request chunks into
	piper *io.PipeReader // read HTTP request chunks from
	bufr  *bufio.Reader  // reads from piper
	close chan struct{}  // closing tells connection handler to exit
}

var (
	// prepared frame
	closeConnectionCommand = &flowd.Frame{
		//Port:     "OUT",
		Type:     "data",
		BodyType: "CloseConnection",
		Extensions: map[string]string{
			"conn-id": "",
		},
	}
	netin *bufio.Reader // NOTE: not for concurrent use, but only for buffer checking
)

func main() {
	// get configuration from arguments = Unix IIP
	unixfbp.DefFlags()
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "ERROR: unexpected free argument(s) encountered")
		flag.PrintDefaults()
		os.Exit(2)
	}

	// connect to FBP network
	var err error
	netout, _, err := unixfbp.OpenOutPort("OUT")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	defer netout.Flush()

	// prepare variables
	var frame *flowd.Frame
	var connID string
	var found bool
	clients := map[string]*Client{} // key = connection ID
	inChan := make(chan *flowd.Frame, 5)
	respChan := make(chan *flowd.Frame, 5)

	// start input port handlers
	go func() {
		// open port IN
		netin, _, err = unixfbp.OpenInPort("IN")
		if err != nil {
			fmt.Println("ERROR:", err)
			os.Exit(2)
		}

		for {
			// read frame
			frame, err = flowd.Deserialize(netin)
			if err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: parsing frame:", err, "- exiting.")
				break
			}
			// send to main loop
			inChan <- frame
		}
	}()
	go func() {
		// open input port RESP
		netresp, _, err := unixfbp.OpenInPort("RESP")
		if err != nil {
			fmt.Println("ERROR:", err)
			os.Exit(2)
		}

		for {
			// read frame
			frame, err = flowd.Deserialize(netresp)
			if err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: parsing frame:", err, "- exiting.")
				break
			}
			// send to main loop
			respChan <- frame
		}
	}()
	// open output port RESP
	respout, _, err := unixfbp.OpenOutPort("RESP")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	if !unixfbp.Quiet {
		fmt.Fprintln(os.Stderr, "serving")
	}

	// main loop
nextframe:
	for {
		select {
		case frame = <-inChan: // IP from network to be sent to HTTP router and/or handlers
			// get connection ID
			//id = frame.Extensions["conn-id"]
			if connID, found = frame.Extensions["conn-id"]; !found {
				fmt.Fprintln(os.Stderr, "WARNING: conn-id header missing on frame:", string(frame.Body), "- discarding.")
				continue
			}

			// handle according to BodyType
			if frame.BodyType == "OpenNotification" {
				fmt.Fprintf(os.Stderr, "%s: got open notification, making new entry.\n", connID)
				// make new entry
				piper, pipew := io.Pipe()
				bufr := bufio.NewReader(piper)
				clients[connID] = &Client{
					piper: piper,
					pipew: pipew,
					bufr:  bufr,
					close: make(chan struct{}, 0),
				}
				// handle request
				go handleConnection(connID, clients[connID], netout)
			} else if frame.BodyType == "CloseNotification" {
				fmt.Fprintf(os.Stderr, "%s: got close notification, removing.\n", connID)
				// send notification to handler goroutine
				close(clients[connID].close)
				// unlink entry
				delete(clients, connID)
			} else {
				// normal data/request frame; send frame body to correct ReadWriter
				fmt.Fprintf(os.Stderr, "%s: got request chunk, feeding to HTTP request parser.\n", connID)
				clients[connID].pipew.Write(frame.Body)
			}
		case frame = <-respChan: // IP from a handler to be sent back to network
			// get connection ID
			//TODO change to use req-id and translate req-id -> conn-id
			if connID, found = frame.Extensions["conn-id"]; !found {
				fmt.Fprintln(os.Stderr, "WARNING: conn-id header missing on frame:", string(frame.Body), "- discarding.")
				continue
			}
			fmt.Fprintf(os.Stderr, "%s: got HTTP response\n", connID)

			// create HTTP response
			resp := &http.Response{
				ProtoMajor: 1,
				ProtoMinor: 1,
				Header:     http.Header{},
			}
			//TODO add support for cookies
			for key, value := range frame.Extensions {
				// special fields
				switch key {
				case "conn-id", "req-id":
					// do not put those into the HTTP response header
					continue
				case "http-status":
					if statusCode, err := strconv.Atoi(value); err != nil {
						fmt.Fprintf(os.Stderr, "%s: WARNING: HTTP status code could not be parsed: %v - discarding.\n", connID, err)
						continue nextframe
					} else {
						resp.StatusCode = statusCode
					}
				default:
					// copy all others to HTTP response header
					resp.Header.Add(key, value)
				}
			}
			resp.Body = ioutil.NopCloser(bytes.NewBuffer(frame.Body))
			resp.ContentLength = int64(len(frame.Body))
			if resp.StatusCode == 0 {
				fmt.Fprintf(os.Stderr, "%s: WARNING: response IP contained no HTTP status code - assuming 200.\n", connID)
				resp.StatusCode = 200
			}

			// convert to []byte
			respBytes := &bytes.Buffer{}
			if err := resp.Write(respBytes); err != nil {
				fmt.Fprintf(os.Stderr, "%s: WARNING: error serializing HTTP response: %v - discarding.\n", connID, err)
				continue
			}

			// package up into frame
			respFrame := &flowd.Frame{
				//Port:     "RESP",
				Type:     "data",
				BodyType: "HTTPResponse",
				Extensions: map[string]string{
					"conn-id": connID,
				},
				Body: respBytes.Bytes(),
			}

			// send to network
			if err := respFrame.Serialize(respout); err != nil {
				fmt.Fprintf(os.Stderr, "%s: ERROR: marshaling HTTP response frame downstream: %v - dropping.\n", connID, err)
			}
			if netin.Buffered() == 0 {
				if err := respout.Flush(); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
				}
			}
			fmt.Fprintf(os.Stderr, "%s: response forwarded\n", connID)
		}
	}
}

func handleConnection(connID string, client *Client, netout *bufio.Writer) {
	// read HTTP request from series of frames
	var req *http.Request
	var err error
	reqDone := make(chan struct{}, 0)
	go func() {
		req, err = http.ReadRequest(client.bufr)
		close(reqDone)
	}()

	// wait for some result
	select {
	case <-reqDone: // read done, maybe with errors
		// check for error
		if err != nil {
			fmt.Fprintf(os.Stderr, "%s: ERROR: could not parse HTTP request: %v - closing.\n", connID, err)
			// request connection be closed
			closeConnectionCommand.Extensions["conn-id"] = connID
			closeConnectionCommand.Serialize(netout)
			// done
			return
		}
		fmt.Fprintf(os.Stderr, "%s: HTTP request complete\n", connID)
		// send parsed HTTP request downstream
		body, _ := ioutil.ReadAll(req.Body)
		fmt.Fprintf(os.Stderr, "%s: request body complete\n", connID)
		reqFrame := &flowd.Frame{
			//Port:     "OUT",
			Type:     "data",
			BodyType: "HTTPRequest",
			Extensions: map[string]string{
				"req-id":  "666", //TODO translation between conn-id and req-id -> multiple requests over one connection
				"conn-id": connID,
			},
			Body: body,
		}
		if err := reqFrame.Serialize(netout); err != nil {
			fmt.Fprintf(os.Stderr, "%s: ERROR: marshaling HTTP request frame downstream: %v - dropping.\n", connID, err)
		}
		if netin.Buffered() == 0 {
			if err := netout.Flush(); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
			}
		}
		fmt.Fprintf(os.Stderr, "%s: request forwarded\n", connID)
	case <-time.After(timeout): // timeout
		fmt.Fprintf(os.Stderr, "%s: WARNING: timeout receiving HTTP request - closing connection.\n", connID)
		client.pipew.Close()
		return
	case <-client.close: // close command from main loop
		client.pipew.Close()
	}
}
