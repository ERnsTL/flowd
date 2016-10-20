package main

import (
	"bufio"
	"fmt"
	"io"
	"net"
	"os"
	"strconv"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// open connection to network
	stdin := bufio.NewReader(os.Stdin)
	// get parameters
	if len(os.Args) < 1+1 {
		// require configuration on cmdline
		/*
			fmt.Fprintln(os.Stderr, "Usage:", os.Args[0], "[host]:[port]")
			os.Exit(1)
		*/

		// set default values if no configuration from cmdline
		/*
			os.Args = []string{"", "localhost:4000"}
			fmt.Fprintln(os.Stderr, "WARNING: No listen address given, using default", os.Args[1])
		*/

		// get configuration from IIP = initial information packet/frame
		fmt.Fprintln(os.Stderr, "wait for IIP")
		if iip, err := flowd.GetIIP("CONF", stdin); err != nil {
			fmt.Println("ERROR getting IIP:", err, "- Exiting.")
			os.Exit(1)
		} else {
			os.Args = []string{"", iip}
		}
	}

	// list of established TCP connections
	conns := make(map[int]*net.TCPConn)

	fmt.Fprintln(os.Stderr, "resolve address")
	serverAddr, err := net.ResolveTCPAddr("tcp", os.Args[1])
	CheckError(err)

	fmt.Fprintln(os.Stderr, "open socket")
	listener, err := net.ListenTCP("tcp", serverAddr)
	CheckError(err)

	// pre-declare often-used IPs/frames
	closeNotification := flowd.Frame{
		Port:     "OUT",
		Type:     "data", //TODO could be marked as "control", but control should probably only be FBP-level control, not application-level control
		BodyType: "CloseNotification",
		Extensions: map[string]string{
			"Tcp-Id": "",
			//"Tcp-Remote-Address": "",
		},
	}
	//TODO is this really necessary? components could also simply check in local state (which they have to keep anyway) if connection is known or a new one
	openNotification := flowd.Frame{
		Port:     "OUT",
		Type:     "data",
		BodyType: "OpenNotification",
		Extensions: map[string]string{
			"Tcp-Id":             "",
			"Tcp-Remote-Address": "",
		},
	}

	// handle responses from STDIN = from FBP network to TCP sockets
	go func(stdin *bufio.Reader) {
		//TODO what if there is no data waiting on STDIN? or if it is closed? would probably get EOF on Read, but check.
		// handle regular packets
		for {
			//TODO what if no complete frame has been received? -> try again later instead of closing.
			//stdin := bufio.NewReader(os.Stdin)
			if frame, err := flowd.ParseFrame(stdin); err != nil {
				if err == io.EOF {
					fmt.Fprintln(os.Stderr, "EOF from FBP network on STDIN. Exiting.")
				} else {
					fmt.Fprintln(os.Stderr, "ERROR parsing frame from FBP network on STDIN:", err, "- Exiting.")
					//TODO notification feedback into FBP network
				}
				os.Stdin.Close()
				//TODO gracefully shut down / close all connections
				os.Exit(0) // TODO exit with non-zero code if error parsing frame
				return
			} else { // frame complete now
				//TODO if debug -- add flag
				//fmt.Fprintln(os.Stderr, "tcp-server: frame in with", bodyLen, "bytes body")
				/*
					//TODO add debug flag; TODO add proper flag parsing
					if debug {
						fmt.Println("STDOUT received frame type", frame.Type, "data type", frame.BodyType, "for port", frame.Port, "with body:", (string)(*frame.Body))
					}
				*/

				//TODO check for non-data/control frames

				///FIXME send close notification downstream also in error cases (we close conn) or if client shuts down connection (EOF)

				//TODO error feedback for unknown/unconnected/closed TCP connections
				// check if frame has any extension headers at all
				if frame.Extensions == nil {
					fmt.Fprintln(os.Stderr, "ERROR: frame is missing extension headers - Exiting.")
					//TODO gracefully shut down / close all connections
					os.Exit(1)
				}
				// check if frame has TCP-ID in header
				if connIdStr, exists := frame.Extensions["Tcp-Id"]; exists {
					// check if TCP-ID header is integer number
					if connId, err := strconv.Atoi(connIdStr); err != nil {
						// TCP-ID header not an integer number
						//TODO notification back to FBP network of error
						fmt.Fprintf(os.Stderr, "ERROR: frame has non-integer Tcp-Id header %s: %s - Exiting.\n", connIdStr, err)
						//TODO gracefully shut down / close all connections
						os.Exit(1)
					} else {
						// check if there is a TCP connection known for that TCP-ID
						if conn, exists := conns[connId]; exists { // found connection
							// write frame body out to TCP connection
							if bytesWritten, err := conn.Write(frame.Body); err != nil {
								//TODO check for EOF
								fmt.Fprintf(os.Stderr, "net out: ERROR writing to TCP connection with %s: %s - closing.\n", conn.RemoteAddr(), err)
								//TODO gracefully shut down / close all connections
								os.Exit(1)
							} else if bytesWritten < len(frame.Body) {
								// short write
								fmt.Fprintf(os.Stderr, "net out: ERROR: short send to TCP connection with %s, only %d of %d bytes written - closing.\n", conn.RemoteAddr(), bytesWritten, len(frame.Body))
								//TODO gracefully shut down / close all connections
								os.Exit(1)
							} else {
								// success
								//TODO if !quiet - add that flag
								fmt.Fprintf(os.Stderr, "%d: wrote %d bytes to %s\n", connId, bytesWritten, conn.RemoteAddr())
							}

							if frame.BodyType == "TCPCloseConnection" {
								fmt.Fprintf(os.Stderr, "%d: got close command, closing connection.\n", connId)
								// close command received, close connection
								conn.Close()
								// NOTE: will be cleaned up on next conn.Read() in handleConnection()
							}
						} else {
							// TCP connection not found - could have been closed in meantime or wrong TCP-ID in frame header
							//TODO notification back to FBP network of undeliverable message
							//TODO gracefully shut down / close all connections
							os.Exit(1)
						}
					}
				} else {
					// TCP-ID extension header missing
					fmt.Fprintln(os.Stderr, "ERROR: frame is missing Tcp-Id header - Exiting.")
					//TODO gracefully shut down / close all connections
					os.Exit(1)
				}
			}
		}
	}(stdin)

	// handle close notifications -> delete connection from map
	closeChan := make(chan int)
	go func() {
		var id int
		for {
			id = <-closeChan
			fmt.Fprintln(os.Stderr, "closer: deleting connection", id)
			delete(conns, id)
			// send close notification downstream
			//TODO with reason (error or closed from other side/this side)
			closeNotification.Extensions["Tcp-Id"] = strconv.Itoa(id)
			closeNotification.Marshal(os.Stdout)
		}
	}()

	// handle TCP connections
	fmt.Fprintln(os.Stderr, "listening...")
	var id int
	for {
		conn, err := listener.AcceptTCP()
		CheckError(err)
		fmt.Fprintln(os.Stderr, "accepted connection from", conn.RemoteAddr())
		conn.SetKeepAlive(true)
		conn.SetKeepAlivePeriod(5 * time.Second)
		conns[id] = conn
		// send new-connection notification downstream
		openNotification.Extensions["Tcp-Id"] = strconv.Itoa(id)
		openNotification.Extensions["Tcp-Remote-Address"] = fmt.Sprintf("%v", conn.RemoteAddr().(*net.TCPAddr)) // copied from handleConnection()
		openNotification.Marshal(os.Stdout)
		// handle connection
		go handleConnection(conn, id, closeChan)
		//TODO overflow possibilities?
		id += 1
	}
}

func CheckError(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR:", err)
		os.Exit(2)
	}
}

func handleConnection(conn *net.TCPConn, id int, closeChan chan int) {
	// prepare data structures
	buf := make([]byte, 65535)
	outframe := flowd.Frame{
		Type:        "data",
		BodyType:    "TCPPacket",
		Port:        "OUT",
		ContentType: "application/octet-stream",
		Extensions:  map[string]string{"Tcp-Id": strconv.Itoa(id), "Tcp-Remote-Address": fmt.Sprintf("%v", conn.RemoteAddr().(*net.TCPAddr))},
		Body:        nil,
	}

	// process TCP packets
	for {
		bytesRead, err := conn.Read(buf)
		if err != nil || bytesRead < 0 {
			// EOF or other error
			//TODO check for EOF specifically by type assertion
			fmt.Fprintf(os.Stderr, "%d: ERROR reading from %v: %s - closing.\n", id, conn.RemoteAddr(), err)
			if err := conn.Close(); err != nil {
				fmt.Fprintf(os.Stderr, "%d: ERROR closing connection: %s\n", id, err)
				//TODO exit whole program? - something is wrong in that situation
			}
			// remove conn from list of connections
			closeChan <- id
			// exit
			return
		}
		//TODO if debug - add flag
		//fmt.Fprintf(os.Stderr, "%d: read %d bytes from %s: %s\n", id, bytesRead, conn.RemoteAddr(), buf[:bytesRead])
		//TODO if !quiet - add flag
		fmt.Fprintf(os.Stderr, "%d: read %d bytes from %s\n", id, bytesRead, conn.RemoteAddr())

		// frame TCP packet into flowd frame
		outframe.Body = buf[:bytesRead]

		// send it to STDOUT = FBP network
		outframe.Marshal(os.Stdout)
	}
}
