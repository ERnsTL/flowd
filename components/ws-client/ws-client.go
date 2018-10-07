package main

import (
	"bytes"
	"flag"
	"fmt"
	"io"
	"log"
	"net/url"
	"os"
	"os/signal"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
	"github.com/gorilla/websocket"
)

//TODO implement in proper FBP fashion running before an http-client component
//TODO add heartbeat ping and pong messages and do not send own ones if peer recently already sent one

const bufSize = 65536

func main() {
	// get configuration from arguments = Unix IIP
	var retry, bridge bool
	unixfbp.DefFlags()
	flag.BoolVar(&bridge, "bridge", false, "bridge mode, true = forward frames from/to FBP network, false = send frame body over WS, frame data from WS")
	flag.BoolVar(&retry, "retry", false, "retry connection and try to reconnect (TODO reconnect currently unimplemented)")
	flag.Parse()
	if flag.NArg() != 1 {
		fmt.Fprintln(os.Stderr, "ERROR: missing remote address - exiting.")
		printUsage()
		flag.PrintDefaults()
		os.Exit(2)
	}
	// connect to FBP network
	//TODO check if nothing connected -> emit warning on -debug and allow one-directional receiving/sending of data
	netin, _, err := unixfbp.OpenInPort("IN")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	netout, _, err := unixfbp.OpenOutPort("OUT")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	defer netout.Flush()

	//TODO implement re-connection

	// parse remote address as URL
	// NOTE: add double slashes after semicolon so that host:port is put into .Host
	remoteURL, err := url.ParseRequestURI(flag.Args()[0])
	checkError(err)

	// TODO refactor to actually be able to retry connections and try to reconnect
	fmt.Fprintln(os.Stderr, "connecting...")
	var conn *websocket.Conn
	for {
		conn, _, err = websocket.DefaultDialer.Dial(remoteURL.String(), nil) // NOTE: second parameter would be the http.Response
		if err == nil {
			break
		}
		time.Sleep(time.Second)
		fmt.Fprintln(os.Stderr, "retrying")
	}
	//checkError(err)
	defer conn.Close()
	fmt.Fprintln(os.Stderr, "connected")
	//TODO notify downstream components using OpenNotification, see ws-server

	// handle interrupt -> send WebSocket close message
	interrupt := make(chan os.Signal, 1)
	signal.Notify(interrupt, os.Interrupt)

	// main loops
	if bridge {
		fmt.Fprintln(os.Stderr, "forwarding frames (bridge)")
	} else {
		fmt.Fprintln(os.Stderr, "forwarding frame bodies")
	}

	closeChan := make(chan bool)

	// handle WebSocket-> FBP
	go func() {
		// prepare data structures
		outframe := flowd.Frame{
			Type:     "data",
			BodyType: "WSPacket",
			//Port:     "OUT",
			//ContentType: "application/octet-stream",
			Extensions: nil,
			Body:       nil,
		}

		// process WS messages
		var msgBytes []byte
		var msgType int
		for {
			msgType, msgBytes, err = conn.ReadMessage()
			if msgType == websocket.CloseMessage {
				log.Println("got close message")
			}
			if err != nil {
				if websocket.IsUnexpectedCloseError(err, websocket.CloseNormalClosure, websocket.CloseGoingAway, websocket.CloseServiceRestart) {
					log.Println("ERROR on WS->FBP reading from WS:", err)
				} else {
					fmt.Fprintln(os.Stderr, "server closed connection normally")
				}
				// close connection and notify main loop
				closeChan <- true
				break
			}
			if unixfbp.Debug {
				fmt.Fprintf(os.Stderr, "ws in: read msg with %d bytes from %s: %s\n", len(msgBytes), conn.RemoteAddr(), msgBytes)
			} else if !unixfbp.Quiet {
				fmt.Fprintf(os.Stderr, "ws in: read msg with %d bytes from %s\n", len(msgBytes), conn.RemoteAddr())
			}

			if bridge {
				// forward raw WS message body, presumably already framed
				_, err = netout.Write(msgBytes)
				if err != nil {
					log.Println("ERROR on WS->FBP forwarding to FBP:", err)
					// close connection and notify main loop
					closeChan <- true
					break
				}
			} else {
				// frame WebSocket message body into flowd frame
				outframe.Body = msgBytes

				// send it to netout = FBP network
				outframe.Serialize(netout)
			}
		}
		fmt.Fprintln(os.Stderr, "WS->FBP exited")
	}()

	// handle FBP -> WebSocket
	go func() {
		var frame *flowd.Frame
		for {
			frame, err = flowd.Deserialize(netin)
			if err != nil {
				if err == io.EOF {
					fmt.Fprintln(os.Stderr, "ws out: EOF from FBP network - shutting down.")
					// shut down gracefully
					closeChan <- true
					break
				} else {
					fmt.Fprintln(os.Stderr, "ws out: ERROR parsing frame from FBP network:", err, "- Exiting.")
					//TODO notification feedback into FBP network
				}
				//FIXME close netin here
				//TODO gracefully shut down / close all connections
				os.Exit(3)
				return
			}

			if unixfbp.Debug {
				fmt.Fprintln(os.Stderr, "ws out: received frame type", frame.Type, "data type", frame.BodyType, "for port", frame.Port, "with body:", string(frame.Body))
			} else if !unixfbp.Quiet {
				fmt.Fprintln(os.Stderr, "ws out: frame in with", len(frame.Body), "bytes body")
			}

			//TODO ability to forward CloseConnection commands in bridge scenario = close connection in some other part of the FBP network?
			if frame.BodyType == "CloseConnection" {
				fmt.Fprintln(os.Stderr, "got close command, closing connection.")
				// shut down gracefully
				close(closeChan)
				break
			}

			if bridge {
				// write frame out to WS connection
				buf := bytes.Buffer{}       //TODO optimize allocations
				err = frame.Serialize(&buf) //TODO optimize: any chance to save deserializing and re-serializing (and into buffer and into WS)?
				if err != nil {
					fmt.Fprintf(os.Stderr, "ws out: ERROR re-serializing frame: %s - closing.\n", err)
					closeChan <- true
					break
				}
				err = conn.WriteMessage(websocket.BinaryMessage, buf.Bytes())
				if err != nil {
					fmt.Fprintf(os.Stderr, "ws out: ERROR writing to WS connection with %s: %s - closing.\n", conn.RemoteAddr(), err)
					closeChan <- true
					break
				}
				// success
				if !unixfbp.Quiet {
					fmt.Fprintf(os.Stderr, "wrote msg with %d bytes frame to %s\n", buf.Len(), conn.RemoteAddr())
				}
			} else {
				// write frame body out to WS connection
				if err = conn.WriteMessage(websocket.BinaryMessage, frame.Body); err != nil {
					//TODO check for EOF
					fmt.Fprintf(os.Stderr, "ws out: ERROR writing to WS connection with %s: %s - closing.\n", conn.RemoteAddr(), err)
					//TODO gracefully shut down / close all connections
					closeChan <- true
					break
				} else {
					// success
					if !unixfbp.Quiet {
						fmt.Fprintf(os.Stderr, "wrote msg with %d bytes body to %s\n", len(frame.Body), conn.RemoteAddr())
					}
				}
			}
		}
		fmt.Fprintln(os.Stderr, "FBP->WS exited")
	}()

	// wait for closed connection or interrupt
	select {
	case <-closeChan:
		fmt.Fprintln(os.Stderr, "connection closed - shutting down...")
		break
	case <-interrupt:
		fmt.Fprintln(os.Stderr, "interrupt - shutting down...")
		break
	}
	// cleanly close the connection by sending a close message
	err = conn.WriteMessage(websocket.CloseMessage, websocket.FormatCloseMessage(websocket.CloseNormalClosure, ""))
	if err != nil {
		// ignore error because conn might already have been closed or encountered other error
		if err != websocket.ErrCloseSent {
			log.Println("ERROR sending close message:", err)
		} else {
			fmt.Fprintln(os.Stderr, "connection already properly shut down")
		}
	} else {
		fmt.Fprintln(os.Stderr, "close message sent")
	}
	// wait (with timeout) for the server to close the connection
	select {
	case <-closeChan:
	case <-time.After(time.Second):
	}
	fmt.Fprintln(os.Stderr, "exiting")
}

func checkError(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR:", err)
		os.Exit(2)
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Arguments: [-debug] [-quiet] [-bridge] [-retry] ws://[host]:[port]/[path]")
}
