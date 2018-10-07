package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/url"
	"os"
	"strconv"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
	"github.com/gorilla/websocket"
)

//TODO implement in proper FBP fashion running behind an http-server component
//TODO harden according to real-world example @ https://github.com/gorilla/websocket/blob/master/examples/chat/client.go
//	SetReadLimit, SetReadDeadline, SetWriteDeadline, SetPongHandler, send pings, handle control messages, handle different close scenarios
//TODO add heartbeat ping and pong messages and do not send own ones if peer recently already sent one

const bufSize = 65536

var (
	netout   *bufio.Writer //TODO is that safe for concurrent use?
	upgrader = websocket.Upgrader{
		//ReadBufferSize:  1024, //TODO optimal values? use own buffers or those from the HTTP server (default)?
		//WriteBufferSize: 1024,
	}
	conns      = make(map[int]*websocket.Conn) // list of established WS connections
	nextConnID int
	closeChan  = make(chan int)
)

func main() {
	// get configuration from arguments = Unix IIP
	var bridge bool
	var maxconn int
	unixfbp.DefFlags()
	flag.BoolVar(&bridge, "bridge", false, "bridge mode, true = forward frames from/to FBP network, false = send frame body over socket, frame data from socket")
	flag.IntVar(&maxconn, "maxconn", 0, "maximum number of connections to accept, 0 = unlimited")
	flag.Parse()
	if flag.NArg() != 1 {
		fmt.Fprintln(os.Stderr, "ERROR: missing listen address - exiting.")
		printUsage()
		flag.PrintDefaults()
		os.Exit(2)
	}

	// connect to FBP network
	var err error
	netout, _, err = unixfbp.OpenOutPort("OUT")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	defer netout.Flush()
	netin, _, err := unixfbp.OpenInPort("IN")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}

	//TODO implement
	if bridge {
		fmt.Fprintln(os.Stderr, "ERROR: flag -bridge currently unimplemented - exiting.")
		os.Exit(2)
	}
	//TODO implement
	if maxconn > 0 {
		fmt.Fprintln(os.Stderr, "ERROR: flag -maxconn currently unimplemented - exiting.")
		os.Exit(2)
	}

	// parse listen address as URL
	// NOTE: add double slashes after semicolon so that host:port is put into .Host
	listenURL, err := url.ParseRequestURI(flag.Args()[0])
	checkError(err)
	listenNetwork := listenURL.Scheme
	//fmt.Fprintf(os.Stderr, "Scheme=%s, Opaque=%s, Host=%s, Path=%s\n", listenURL.Scheme, listenURL.Opaque, listenURL.Host, listenURL.Path)
	listenHost := listenURL.Host

	fmt.Fprintln(os.Stderr, "resolve address")
	serverAddr, err := net.ResolveTCPAddr(listenNetwork, listenHost)
	checkError(err)

	fmt.Fprintln(os.Stderr, "starting up")
	// set up logging
	log.SetOutput(os.Stderr)
	// pre-declare often-used IPs/frames
	closeNotification := flowd.Frame{
		//Port:     "OUT",
		Type:     "data", //TODO could be marked as "control", but control should probably only be FBP-level control, not application-level control
		BodyType: "CloseNotification",
		Extensions: map[string]string{
			"conn-id": "",
			//"remote-address": "",
		},
	}

	// handle responses from netin = from FBP network to Websockets
	go func(stdin *bufio.Reader) {
		// handle regular packets
		for {
			frame, err := flowd.Deserialize(stdin)
			if err != nil {
				if err == io.EOF {
					fmt.Fprintln(os.Stderr, "EOF from FBP network. Exiting.")
				} else {
					fmt.Fprintln(os.Stderr, "ERROR: parsing frame from FBP network:", err, "- exiting.")
					//TODO notification feedback into FBP network
				}
				os.Stdin.Close()
				//TODO gracefully shut down / close all connections
				os.Exit(0) // TODO exit with non-zero code if error parsing frame
				return
			}

			if unixfbp.Debug {
				fmt.Fprintln(os.Stderr, "received frame type", frame.Type, "data type", frame.BodyType, "for port", frame.Port, "with body:", string(frame.Body))
			} else if !unixfbp.Quiet {
				fmt.Fprintln(os.Stderr, "frame out with", len(frame.Body), "bytes body")
			}

			//TODO check for non-data/control frames

			//FIXME send close notification downstream also in error cases (we close conn) or if client shuts down connection (EOF)

			//TODO error feedback for unknown/unconnected/closed Websocket connections
			// check if frame has any extension headers at all
			if frame.Extensions == nil {
				fmt.Fprintln(os.Stderr, "ERROR: frame is missing extension headers - Exiting.")
				//TODO gracefully shut down / close all connections
				os.Exit(1)
			}
			// check if frame has conn-id in header
			if connIDStr, exists := frame.Extensions["conn-id"]; exists {
				// check if conn-id header is integer number
				if connID, err := strconv.Atoi(connIDStr); err != nil {
					// conn-id header not an integer number
					//TODO notification back to FBP network of error
					fmt.Fprintf(os.Stderr, "ERROR: frame has non-integer conn-id header %s: %s - Exiting.\n", connIDStr, err)
					//TODO gracefully shut down / close all connections
					os.Exit(1)
				} else {
					// check if there is a Websocket connection known for that conn-id
					if conn, exists := conns[connID]; exists { // found connection
						// write frame body out to Websocket connection
						if err := conn.WriteMessage(websocket.BinaryMessage, frame.Body); err != nil {
							//TODO check for EOF
							fmt.Fprintf(os.Stderr, "net out: ERROR writing to WS connection with %s: %s - closing.\n", conn.RemoteAddr(), err)
							//TODO gracefully shut down / close all connections
							os.Exit(1)
						} else {
							// success
							//TODO if !quiet - add that flag
							fmt.Fprintf(os.Stderr, "%d: wrote message with %d bytes body to %s\n", connID, len(frame.Body), conn.RemoteAddr())
						}
						if frame.BodyType == "CloseConnection" {
							fmt.Fprintf(os.Stderr, "%d: got close command, closing connection.\n", connID)
							// close command received, close connection
							conn.Close()
							// NOTE: will be cleaned up on next conn.Read() in handleConnection()
						}
					} else {
						// WS connection not found - could have been closed in meantime or wrong conn-id in frame header
						//TODO notification back to FBP network of undeliverable message
						//TODO gracefully shut down / close all connections
						if frame.BodyType == "CloseNotification" {
							fmt.Fprintf(os.Stderr, "%d: received back own close notification - discarding.\n", connID)
							continue
						}
						fmt.Fprintf(os.Stderr, "ERROR: got frame for unknown connection: %d - Exiting.\n", connID)
						os.Exit(1)
					}
				}
			} else {
				// conn-id extension header missing
				fmt.Fprintln(os.Stderr, "ERROR: frame is missing conn-id header - Exiting.")
				//TODO gracefully shut down / close all connections
				os.Exit(1)
			}
		}
	}(netin)

	// handle close notifications -> delete connection from map
	go func() {
		var id int
		for {
			id = <-closeChan
			if !unixfbp.Quiet {
				fmt.Fprintln(os.Stderr, "closer: deleting connection", id)
			}
			delete(conns, id)
			// send close notification downstream
			//TODO with reason (error or closed from other side/this side)
			closeNotification.Extensions["conn-id"] = strconv.Itoa(id)
			closeNotification.Serialize(netout)
			// flush buffers = send frames
			if err := netout.Flush(); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
			}
		}
	}()

	fmt.Fprintln(os.Stderr, "listen and serve...")
	http.HandleFunc(listenURL.Path, handleWSConnection)
	log.Fatal(http.ListenAndServe(serverAddr.String(), nil)) // NOTE: second argument would be a handler
}

func checkError(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR:", err)
		os.Exit(2)
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Arguments: [flags] [tcp|tcp4|tcp6]://[host][:port]/[websocket-path]")
}

// pre-allocate often-used variables
/* NOTE:
OpenNotification is used to inform downstream component(s) of a new connection
and - once - send which address and port is on the other side. Downstream components
can check, save etc. the address information, but for sending responses, the
conn-id header is relevant.
*/
var openNotification = flowd.Frame{
	//Port:     "OUT",
	Type:     "data",
	BodyType: "OpenNotification",
	Extensions: map[string]string{
		"conn-id":        "",
		"remote-address": "",
	},
}

func handleWSConnection(w http.ResponseWriter, r *http.Request) {
	// upgrade from HTTP to WebSocket
	if !unixfbp.Quiet {
		fmt.Fprintln(os.Stderr, "accepted connection from", r.RemoteAddr, "- upgrading...")
	}
	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		log.Print("WARNING: upgrade failed:", err, "- closing.") //TODO add remote address etc.
		return
	}
	// generate connection ID
	id := nextConnID
	nextConnID++
	// save connection
	//TODO add locking
	conns[id] = conn
	defer conn.Close() // is being closed here, entry in conns is removed in other Goroutine
	if !unixfbp.Quiet {
		fmt.Fprintf(os.Stderr, "%d: new WS connection from %s\n", id, r.RemoteAddr)
	}

	// send new-connection notification downstream
	openNotification.Extensions["conn-id"] = strconv.Itoa(id)
	openNotification.Extensions["remote-address"] = r.RemoteAddr
	openNotification.Serialize(netout)
	// flush buffer = send frame
	if err = netout.Flush(); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
	}

	// handle WS messages
	var outframe = flowd.Frame{
		Type:     "data",
		BodyType: "WSPacket",
		//Port:     "OUT",
		//ContentType: "application/octet-stream",
		Extensions: map[string]string{"conn-id": strconv.Itoa(id)},
		Body:       nil,
	}
	for {
		msgType, msgBody, err := conn.ReadMessage() //TODO optimize allocations
		if err != nil {
			if websocket.IsCloseError(err, websocket.CloseNormalClosure, websocket.CloseGoingAway) {
				fmt.Fprintf(os.Stderr, "%d: client requesting normal connection closure - closing.\n", id)
			} else {
				fmt.Fprintf(os.Stderr, "%d: WARNING: reading message failed: %s - closing.\n", id, err)
			}
			// close connection
			//_ = conn.Close()
			// remove conn from list of connections
			closeChan <- id
			break
		}
		if unixfbp.Debug {
			fmt.Fprintf(os.Stderr, "%d: msg in with %d bytes body from %s: %s\n", id, len(msgBody), r.RemoteAddr, msgBody)
		} else if !unixfbp.Quiet {
			fmt.Fprintf(os.Stderr, "%d: msg in with %d bytes body from %s\n", id, len(msgBody), r.RemoteAddr)
		}

		// handle message
		switch msgType {
		case websocket.TextMessage, websocket.BinaryMessage:
			// frame WS packet into flowd frame
			outframe.Body = msgBody

			// send it to STDOUT = FBP network
			outframe.Serialize(netout)
			// send it now (flush)
			if err = netout.Flush(); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
			}
			if !unixfbp.Quiet {
				fmt.Fprintf(os.Stderr, "%d: forwarded\n", id)
			}
		case websocket.CloseMessage: // TODO:
			log.Fatalf("%d: got close message - unimplemented", id)
		case websocket.PingMessage, websocket.PongMessage:
			log.Fatalf("%d: got ping|pong message - unimplemented", id)
		default:
			log.Printf("%d: unexpected WS message type: %d - closing.\n", id, msgType)

			// close and remove
			//_ = conn.Close()
			closeChan <- id
		}
	}
}
