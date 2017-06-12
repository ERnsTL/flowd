package main

import (
	"bufio"
	"fmt"
	"io"
	"io/ioutil"
	"net/http"
	"os"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
)

const timeout = 10 * time.Second

// Client is .... TODO
type Client struct {
	// data exchange between main loop and connection handler
	piper *io.PipeReader
	pipew *io.PipeWriter
	bufr  *bufio.Reader // reads from piper
	close chan struct{}
}

// global variables
//TODO useful?
var (
	closeCommand = &flowd.Frame{
		Port:     "OUT",
		Type:     "data",
		BodyType: "CloseConnection",
		Extensions: map[string]string{
			"conn-id": "",
		},
	}
)

func main() {
	// prepare commonly-used variables
	bufr := bufio.NewReader(os.Stdin)
	var frame *flowd.Frame //TODO why is this pointer of Frame?
	var err error
	var connID string
	var found bool
	clients := map[string]*Client{} // key = connection ID

	for {
		// read frame
		frame, err = flowd.ParseFrame(bufr)
		if err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: parsing frame:", err, "- exiting.")
			break
		}

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
			go handleConnection(connID, clients[connID])
		} else if frame.BodyType == "CloseNotification" {
			fmt.Fprintf(os.Stderr, "%s: got close notification, removing.\n", connID)
			// send notification to handler goroutine
			close(clients[connID].close)
			// unlink entry
			delete(clients, connID)
		} else {
			// normal data/request frame; send frame body to correct ReadWriter
			clients[connID].pipew.Write(frame.Body)
		}
	}
}

func handleConnection(connID string, client *Client) {
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
			fmt.Fprintf(os.Stderr, "%s: ERROR: could not parse HTTP request: %v - closing.", connID, err)
			// request connection be closed
			closeCommand.Extensions["conn-id"] = connID
			closeCommand.Marshal(os.Stdout)
			// done
			return
		}
		// send parsed HTTP request downstream
		body, _ := ioutil.ReadAll(req.Body)
		reqFrame := &flowd.Frame{
			Port:     "OUT",
			Type:     "data",
			BodyType: "HTTPRequest",
			Extensions: map[string]string{
				"Http-Id": "666", //TODO
			},
			Body: body, //TODO
		}
		if err := reqFrame.Marshal(os.Stdout); err != nil {
			fmt.Fprintf(os.Stderr, "%s: ERROR: marshaling HTTP request frame downstream: %v - dropping.", connID, err)
		}
		// TODO multiple requests
		client.pipew.Close()
	case <-time.After(timeout): // timeout
		client.pipew.Close()
		return
	case <-client.close: // close command from main loop
		client.pipew.Close()
	}
}
