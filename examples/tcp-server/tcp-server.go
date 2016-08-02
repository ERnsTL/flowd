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
	if len(os.Args) < 1+1 {
		fmt.Fprintln(os.Stderr, "Usage:", os.Args[0], "[host]:[port]")
		os.Exit(1)
	}

	conns := make(map[int]*net.TCPConn)

	// STDIN: packets to send out (responses)

	fmt.Fprintln(os.Stderr, "resolving address")
	serverAddr, err := net.ResolveTCPAddr("tcp", os.Args[1])
	CheckError(err)

	fmt.Fprintln(os.Stderr, "open socket")
	listener, err := net.ListenTCP("tcp", serverAddr)
	CheckError(err)

	// handle responses from STDIN = from FBP network to TCP sockets
	go func() {
		//TODO what if there is no data waiting on STDIN? or if it is closed? would probably get EOF on Read, but check.
		stdin := bufio.NewReader(os.Stdin)
		for {
			//TODO what if no complete frame has been received? -> try again later instead of closing.
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
				bodyLen := len(*frame.Body)
				fmt.Fprintln(os.Stderr, "in xfer", bodyLen, "bytes")
				/*
					//TODO add debug flag; TODO add proper flag parsing
					if debug {
						fmt.Println("STDOUT received frame type", frame.Type, "data type", frame.BodyType, "for port", frame.Port, "with body:", (string)(*frame.Body))
					}
				*/
				//TODO check for non-data/control frames

				//TODO how are connections closed regularly, ie. if client closes it after all is said and done?
				//TODO way for FBP network to close connection after that frame.

				// write out to TCP network
				//TODO error feedback for unknown/unconnected/closed TCP connections
				// check if frame has any extension headers at all
				if frame.Extensions == nil {
					fmt.Fprintln(os.Stderr, "ERROR: frame is missing extension headers - Exiting.")
					//TODO gracefully shut down / close all connections
					os.Exit(1)
				}
				// check if frame has TCP-ID header
				if connIdStr, exists := frame.Extensions["TCP-ID"]; exists {
					// check if TCP-ID header is integer number
					if connId, err := strconv.Atoi(connIdStr); err != nil {
						// TCP-ID header not an integer number
						//TODO notification back to FBP network of error
						fmt.Fprintf(os.Stderr, "ERROR: frame has non-integer TCP-ID %s: %s - Exiting.\n", connIdStr, err)
						//TODO gracefully shut down / close all connections
						os.Exit(1)
					} else {
						// check if there is a TCP connection known for that TCP-ID
						if conn, exists := conns[connId]; exists {
							// found connection, write frame body out
							if bytesWritten, err := conn.Write(*frame.Body); err != nil {
								//TODO check for EOF
								fmt.Fprintf(os.Stderr, "net out: ERROR writing to TCP connection with %s: %s - closing.\n", conn.RemoteAddr(), err)
								//TODO gracefully shut down / close all connections
								os.Exit(1)
							} else if bytesWritten < bodyLen {
								// short write
								fmt.Fprintf(os.Stderr, "net out: ERROR: short send to TCP connection with %s, only %d of %d bytes written - closing.\n", conn.RemoteAddr(), bytesWritten, bodyLen)
								//TODO gracefully shut down / close all connections
								os.Exit(1)
							} else {
								// success
								//TODO if !quiet - add that flag
								fmt.Fprintln(os.Stderr, "net out: wrote", bytesWritten, "bytes to", conn.RemoteAddr())
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
					fmt.Fprintln(os.Stderr, "ERROR: frame is missing TCP-ID header - Exiting.")
					//TODO gracefully shut down / close all connections
					os.Exit(1)
				}
			}
		}
	}()

	// handle close notifications -> delete connection from map
	var closeChan chan int
	go func() {
		var id int
		for {
			id = <-closeChan
			fmt.Fprintln(os.Stderr, "closer: deleting connection", id)
			delete(conns, id)
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
		go handleConnection(conn, id, closeChan)
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
		Extensions:  map[string]string{"TCP-ID": strconv.Itoa(id), "TCP-Remote-Address": fmt.Sprintf("%v", conn.RemoteAddr().(*net.TCPAddr))},
		Body:        nil,
	}

	// process TCP packets
	for {
		bytesRead, err := conn.Read(buf)
		if err != nil || bytesRead < 0 {
			// EOF or other error
			fmt.Fprintf(os.Stderr, "%d: ERROR: reading from connection %v: %s - closing.\n", id, conn, err)
			if err := conn.Close(); err != nil {
				fmt.Fprintf(os.Stderr, "%d: ERROR: closing connection: %s", id, err)
				//TODO exit whole program? - something is wrong in that situation
			}
			// remove conn from list of connections
			closeChan <- id
			// exit
			return
		}
		fmt.Fprintf(os.Stderr, "%d: received: %s\n", id, buf[:bytesRead])

		// frame TCP packet into flowd frame
		//TODO useless copying
		var body []byte
		body = buf[:bytesRead]
		outframe.Body = &body

		// send it to STDOUT = FBP network
		outframe.Marshal(os.Stdout)
	}
}
