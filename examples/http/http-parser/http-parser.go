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
		BodyType: "TCPCloseConnection",
		Extensions: map[string]string{
			"Tcp-Id": "",
		},
	}
)

func main() {
	// prepare commonly-used variables
	bufr := bufio.NewReader(os.Stdin)
	var frame *flowd.Frame //TODO why is this pointer of Frame?
	var err error
	var connId string
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
		//id = frame.Extensions["Tcp-Id"]
		if connId, found = frame.Extensions["Tcp-Id"]; !found {
			fmt.Fprintln(os.Stderr, "WARNING: Tcp-Id header missing on frame:", string(frame.Body), "- discarding.")
			continue
		}

		// handle according to BodyType
		if frame.BodyType == "OpenNotification" {
			fmt.Fprintf(os.Stderr, "%s: got open notification, making new entry.\n", connId)
			// make new entry
			piper, pipew := io.Pipe()
			bufr := bufio.NewReader(piper)
			clients[connId] = &Client{
				piper: piper,
				pipew: pipew,
				bufr:  bufr,
				close: make(chan struct{}, 0),
			}
			// handle request
			go handleConnection(connId, clients[connId])
		} else if frame.BodyType == "CloseNotification" {
			fmt.Fprintf(os.Stderr, "%s: got close notification, removing.\n", connId)
			// send notification to handler goroutine
			close(clients[connId].close)
			// unlink entry
			delete(clients, connId)
		} else {
			// normal data/request frame; send frame body to correct ReadWriter
			clients[connId].pipew.Write(frame.Body)
		}
	}
}

func handleConnection(connId string, client *Client) {
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
			fmt.Fprintf(os.Stderr, "%s: ERROR: could not parse HTTP request: %v - closing.", connId, err)
			// request connection be closed
			closeCommand := &flowd.Frame{
				Port:     "OUT",
				Type:     "data",
				BodyType: "TCPCloseConnection",
				Extensions: map[string]string{
					"Tcp-Id": connId,
				},
			}
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
			fmt.Fprintf(os.Stderr, "%s: ERROR: marshaling HTTP request frame downstream: %v - dropping.", connId, err)
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
