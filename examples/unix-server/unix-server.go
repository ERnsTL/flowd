package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"net"
	"net/url"
	"os"
	"strconv"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
)

const bufSize = 65536

var (
	debug, quiet bool          //TODO is that a good idea? concurrent variable access? performance?
	netout       *bufio.Writer //TODO is that safe for concurrent use?
)

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout = bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	iip, err := flowd.GetIIP("CONF", netin)
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	}
	// parse IIP
	flags := flag.NewFlagSet("unix-server", flag.ContinueOnError)
	var bridge bool
	var maxconn int
	flags.BoolVar(&bridge, "bridge", false, "bridge mode, true = forward frames from/to FBP network, false = send frame body over socket, frame data from socket")
	flags.IntVar(&maxconn, "maxconn", 0, "maximum number of connections to accept, 0 = unlimited")
	flags.BoolVar(&debug, "debug", false, "give detailed event output")
	flags.BoolVar(&quiet, "quiet", false, "no informational output except errors")
	if err = flags.Parse(strings.Split(iip, " ")); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR parsing IIP arguments - exiting.")
		printUsage()
		flags.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	if flags.NArg() != 1 {
		fmt.Fprintln(os.Stderr, "ERROR: missing remote address - exiting.")
		printUsage()
		flags.PrintDefaults()
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
	// NOTE: no double slashes after semicolon, otherwise what is given after that
	// gets put into .Host and .Path and @ (for abstract sockets) cannot be recognized
	listenURL, err := url.ParseRequestURI(flags.Args()[0])
	checkError(err)
	listenNetwork := listenURL.Scheme
	//fmt.Fprintf(os.Stderr, "Scheme=%s, Opaque=%s, Host=%s, Path=%s\n", listenURL.Scheme, listenURL.Opaque, listenURL.Host, listenURL.Path)
	listenPath := listenURL.Opaque
	if listenNetwork == "unixgram" {
		fmt.Fprintln(os.Stderr, "ERROR: network 'unixgram' unimplemented, refer to unixgram-server component - Exiting.") //TODO implement that
		os.Exit(1)
	}

	// list of established connections
	conns := make(map[int]*net.UnixConn)

	fmt.Fprintln(os.Stderr, "resolve address")
	serverAddr, err := net.ResolveUnixAddr(listenNetwork, listenPath)
	checkError(err)

	fmt.Fprintln(os.Stderr, "open socket")
	listener, err := net.ListenUnix(serverAddr.Network(), serverAddr)
	checkError(err)
	//TODO clean up regular filesystem-bound socket after exit (use defer)

	// pre-declare often-used IPs/frames
	closeNotification := flowd.Frame{
		Port:     "OUT",
		Type:     "data", //TODO could be marked as "control", but control should probably only be FBP-level control, not application-level control
		BodyType: "CloseNotification",
		Extensions: map[string]string{
			"conn-id": "",
			//"remote-address": "",
		},
	}
	/* NOTE:
	OpenNotification is used to inform downstream component(s) of a new connection
	and - once - send which address and port is on the other side. Downstream components
	can check, save etc. the address information, but for sending responses, the
	conn-id header is relevant.
	*/
	openNotification := flowd.Frame{
		Port:     "OUT",
		Type:     "data",
		BodyType: "OpenNotification",
		Extensions: map[string]string{
			"conn-id":        "",
			"remote-address": "",
		},
	}

	// handle responses from STDIN = from FBP network to UNIX sockets
	go func(stdin *bufio.Reader) {
		// handle regular packets
		for {
			frame, err := flowd.ParseFrame(netin)
			if err != nil {
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
			}

			if debug {
				fmt.Fprintln(os.Stderr, "received frame type", frame.Type, "data type", frame.BodyType, "for port", frame.Port, "with body:", string(frame.Body))
			} else if !quiet {
				fmt.Fprintln(os.Stderr, "frame in with", len(frame.Body), "bytes body")
			}

			//TODO check for non-data/control frames

			//FIXME send close notification downstream also in error cases (we close conn) or if client shuts down connection (EOF)

			//TODO error feedback for unknown/unconnected/closed UNIX connections
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
					// check if there is a connection known for that conn-id
					if conn, exists := conns[connID]; exists { // found connection
						// write frame body out to UNIX connection
						if bytesWritten, err := conn.Write(frame.Body); err != nil {
							//TODO check for EOF
							fmt.Fprintf(os.Stderr, "net out: ERROR writing to UNIX connection with %s: %s - closing.\n", conn.RemoteAddr(), err)
							//TODO gracefully shut down / close all connections
							os.Exit(1)
						} else if bytesWritten < len(frame.Body) {
							// short write
							fmt.Fprintf(os.Stderr, "net out: ERROR: short send to UNIX connection with %s, only %d of %d bytes written - closing.\n", conn.RemoteAddr(), bytesWritten, len(frame.Body))
							//TODO gracefully shut down / close all connections
							os.Exit(1)
						} else {
							// success
							//TODO if !quiet - add that flag
							fmt.Fprintf(os.Stderr, "%d: wrote %d bytes to %s\n", connID, bytesWritten, conn.RemoteAddr())
						}

						if frame.BodyType == "CloseConnection" {
							fmt.Fprintf(os.Stderr, "%d: got close command, closing connection.\n", connID)
							// close command received, close connection
							conn.Close()
							// NOTE: will be cleaned up on next conn.Read() in handleConnection()
						}
					} else {
						// Connection not found - could have been closed in meantime or wrong conn-id in frame header
						//TODO notification back to FBP network of undeliverable message
						//TODO gracefully shut down / close all connections
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
	closeChan := make(chan int)
	go func() {
		var id int
		for {
			id = <-closeChan
			fmt.Fprintln(os.Stderr, "closer: deleting connection", id)
			delete(conns, id)
			// send close notification downstream
			//TODO with reason (error or closed from other side/this side)
			closeNotification.Extensions["conn-id"] = strconv.Itoa(id)
			closeNotification.Marshal(netout)
			// flush buffers = send frames
			if err = netout.Flush(); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
			}
		}
	}()

	// handle incoming connections
	fmt.Fprintln(os.Stderr, "listening...")
	var id int
	for {
		conn, err := listener.AcceptUnix()
		checkError(err)
		fmt.Fprintln(os.Stderr, "accepted connection from", conn.RemoteAddr())
		conns[id] = conn
		// send new-connection notification downstream
		openNotification.Extensions["conn-id"] = strconv.Itoa(id)
		openNotification.Extensions["remote-address"] = fmt.Sprintf("%v", conn.RemoteAddr()) // copied from handleConnection()
		openNotification.Marshal(netout)
		// flush buffers = send frame
		if err = netout.Flush(); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
		}
		// handle connection
		go handleConnection(conn, id, closeChan)
		//TODO overflow possibilities?
		id++
	}
}

func checkError(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR:", err)
		os.Exit(2)
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "IIP format: [flags] [unix|unixpacket|unixgram]:[@][path|name]")
}

func handleConnection(conn *net.UnixConn, id int, closeChan chan int) {
	// prepare data structures
	buf := make([]byte, bufSize)
	outframe := flowd.Frame{
		Type:     "data",
		BodyType: "UNIXPacket",
		Port:     "OUT",
		//ContentType: "application/octet-stream",
		Extensions: map[string]string{"conn-id": strconv.Itoa(id)}, // NOTE: only on OpenNotification is the "remote-address" header field set
		Body:       nil,
	}

	// process UNIX packets
	for {
		bytesRead, err := conn.Read(buf)
		if err != nil || bytesRead <= 0 {
			//TODO SetReadDeadline?? // Read can be made to time out and return a Error with Timeout() == true
			// after a fixed time limit; see SetDeadline and SetReadDeadline.
			//Read(b []byte) (n int, err error)
			// check more specifically
			if err == io.EOF {
				// EOF = closed by peer or already closed @ STDIN handler goroutine or network error
				fmt.Fprintf(os.Stderr, "%d: EOF on connection, closing.\n", id)
			} else if neterr, isneterr := err.(net.Error); isneterr && neterr.Timeout() {
				// network timeout
				fmt.Fprintf(os.Stderr, "%d: ERROR reading from %v: timeout: %s, closing.\n", id, conn.RemoteAddr(), neterr)
			} else {
				// other error
				fmt.Fprintf(os.Stderr, "%d: ERROR: %s - closing.\n", id, err)
			}
			// close connection
			/*
				NOTE: gives error if already closed by close command @ STDIN handler goroutine
				if err := conn.Close(); err != nil {
					fmt.Fprintf(os.Stderr, "%d: ERROR closing connection: %s\n", id, err)
					//TODO exit whole program? - something is wrong in that situation
				}
			*/
			_ = conn.Close()
			// remove conn from list of connections
			closeChan <- id
			// exit
			return
		}
		//TODO if debug - add flag
		//fmt.Fprintf(os.Stderr, "%d: read %d bytes from %s: %s\n", id, bytesRead, conn.RemoteAddr(), buf[:bytesRead])
		//TODO if !quiet - add flag
		fmt.Fprintf(os.Stderr, "%d: read %d bytes from %s\n", id, bytesRead, conn.RemoteAddr())

		// frame UNIX packet into flowd frame
		outframe.Body = buf[:bytesRead]

		// send it to STDOUT = FBP network
		outframe.Marshal(netout)
		// flush buffers = send frames
		if err := netout.Flush(); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
		}
	}
}
