package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"net"
	"net/url"
	"os"
	"strings"

	flowd "github.com/ERnsTL/flowd/libflowd"
)

const bufSize = 65536

var (
	debug bool
	quiet bool
)

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	iip, err := flowd.GetIIP("CONF", netin)
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- exiting.")
		os.Exit(1)
	}
	// parse IIP
	flags := flag.NewFlagSet("tcp-client", flag.ContinueOnError)
	var retry, bridge bool
	flags.BoolVar(&bridge, "bridge", true, "bridge mode, true = forward frames from/to FBP network, false = send frame body over TCP, frame data from TCP")
	flags.BoolVar(&retry, "retry", false, "retry connection and try to reconnect")
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
	if retry {
		fmt.Fprintln(os.Stderr, "ERROR: flag -retry currently unimplemented - exiting.")
		os.Exit(2)
	}

	// parse remote address as URL
	// NOTE: add double slashes after semicolon so that host:port is put into .Host
	remoteURL, err := url.ParseRequestURI(flags.Args()[0])
	checkError(err)
	remoteNetwork := remoteURL.Scheme
	//fmt.Fprintf(os.Stderr, "Scheme=%s, Opaque=%s, Host=%s, Path=%s\n", listenURL.Scheme, listenURL.Opaque, listenURL.Host, listenURL.Path)
	remoteHost := remoteURL.Host

	fmt.Fprintln(os.Stderr, "resolving addresses")
	serverAddr, err := net.ResolveTCPAddr(remoteNetwork, remoteHost)
	checkError(err)

	localAddr, err := net.ResolveTCPAddr(remoteNetwork, "localhost:0")
	checkError(err)

	// TODO refactor to actually be able to retry connections and try to reconnect
	fmt.Fprintln(os.Stderr, "connecting...")
	conn, err := net.DialTCP(serverAddr.Network(), localAddr, serverAddr)
	checkError(err)
	defer conn.Close()
	fmt.Fprintln(os.Stderr, "connected")
	//TODO notify downstream components using OpenNotification, see tcp-server

	if bridge {
		fmt.Fprintln(os.Stderr, "forwarding frames")
		// copy STDIN to network connection
		go func() {
			_, err := io.Copy(conn, os.Stdin)
			if err != nil {
				fmt.Fprintln(os.Stderr, "ERROR on FBP->TCP:", err)
				return
			}
			fmt.Fprintln(os.Stderr, "FBP->TCP closed")
		}()

		// copy network connection to STDOUT, blocking until EOF
		if _, err := io.Copy(os.Stdout, conn); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR on TCP->FBP:", err)
			return
		}
		fmt.Fprintln(os.Stderr, "TCP->FBP closed")
	} else {
		// send and receive frame bodies
		closeChan := make(chan bool)

		// handle TCP -> FBP
		go handleConnection(conn, closeChan, netout)

		// handle FBP -> TCP
		//TODO pretty much 1:1 copy of tcp-server main loop
		go func() {
			for {
				frame, err := flowd.ParseFrame(netin)
				if err != nil {
					if err == io.EOF {
						fmt.Fprintln(os.Stderr, "tcp out: EOF from FBP network on STDIN. Exiting.")
					} else {
						fmt.Fprintln(os.Stderr, "tcp out: ERROR parsing frame from FBP network on STDIN:", err, "- Exiting.")
						//TODO notification feedback into FBP network
					}
					os.Stdin.Close()
					//TODO gracefully shut down / close all connections
					os.Exit(3)
					return
				}

				if debug {
					fmt.Fprintln(os.Stderr, "tcp out: received frame type", frame.Type, "data type", frame.BodyType, "for port", frame.Port, "with body:", string(frame.Body))
				} else if !quiet {
					fmt.Fprintln(os.Stderr, "tcp out: frame in with", len(frame.Body), "bytes body")
				}

				//TODO check for non-data/control frames
				//FIXME send close notification downstream also in error cases (we close conn) or if client shuts down connection (EOF)
				//TODO error feedback for unknown/unconnected/closed TCP connections

				// write frame body out to TCP connection
				if bytesWritten, err := conn.Write(frame.Body); err != nil {
					//TODO check for EOF
					fmt.Fprintf(os.Stderr, "tcp out: ERROR writing to TCP connection with %s: %s - closing.\n", conn.RemoteAddr(), err)
					//TODO gracefully shut down / close all connections
					os.Exit(1)
				} else if bytesWritten < len(frame.Body) {
					// short write
					fmt.Fprintf(os.Stderr, "tcp out: ERROR: short send to TCP connection with %s, only %d of %d bytes written - closing.\n", conn.RemoteAddr(), bytesWritten, len(frame.Body))
					//TODO gracefully shut down / close all connections
					os.Exit(1)
				} else {
					// success
					//TODO if !quiet - add that flag
					fmt.Fprintf(os.Stderr, "tcp out: wrote %d bytes to %s\n", bytesWritten, conn.RemoteAddr())
				}

				if frame.BodyType == "CloseConnection" {
					fmt.Fprintln(os.Stderr, "got close command, closing connection.")
					// close command received, close connection
					conn.Close()
					//TODO send into closeChan, dont close it here
				}
			}
		}()

		// wait for connection close
		//TODO notify downstream components using CloseNotification, see tcp-server
		<-closeChan
		<-closeChan
	}

	// all done
	fmt.Fprintln(os.Stderr, "done, exiting")
}

func checkError(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR:", err)
		os.Exit(2)
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "IIP format: [-debug] [-quiet] [-bridge] [-retry] [host]:[port]")
}

//TODO optimize: give netout as parameter or use global variable? safe for concurrent use?
//TODO pretty much 1:1 copy from tcp-server handleConnection() -> reuse?
func handleConnection(conn *net.TCPConn, closeChan chan<- bool, netout *bufio.Writer) {
	// prepare data structures
	buf := make([]byte, bufSize)
	outframe := flowd.Frame{
		Type:     "data",
		BodyType: "TCPPacket",
		Port:     "OUT",
		//ContentType: "application/octet-stream",
		Extensions: nil,
		Body:       nil,
	}

	// process TCP packets
	for {
		bytesRead, err := conn.Read(buf)
		if err != nil || bytesRead <= 0 {
			//TODO SetReadDeadline?? // Read can be made to time out and return a Error with Timeout() == true
			// after a fixed time limit; see SetDeadline and SetReadDeadline.
			//Read(b []byte) (n int, err error)
			// NOTE: source @ https://stackoverflow.com/questions/12741386/how-to-know-tcp-connection-is-closed-in-golang-net-package
			// check more specifically
			if err == io.EOF {
				// EOF = closed by peer or already closed @ STDIN handler goroutine or network error
				fmt.Fprintln(os.Stderr, "tcp in: EOF on connection, closing.")
			} else if neterr, isneterr := err.(net.Error); isneterr && neterr.Timeout() {
				// network timeout
				fmt.Fprintf(os.Stderr, "tcp in: ERROR reading from %v: timeout: %s, closing.\n", conn.RemoteAddr(), neterr)
			} else {
				// other error
				fmt.Fprintf(os.Stderr, "tcp in: ERROR: %s - closing.\n", err)
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
			closeChan <- true
			// exit
			return
		}

		if debug {
			fmt.Fprintf(os.Stderr, "tcp in: read %d bytes from %s: %s\n", bytesRead, conn.RemoteAddr(), buf[:bytesRead])
		} else if !quiet {
			fmt.Fprintf(os.Stderr, "tcp in: read %d bytes from %s\n", bytesRead, conn.RemoteAddr())
		}

		// frame TCP packet into flowd frame
		outframe.Body = buf[:bytesRead]

		// send it to STDOUT = FBP network
		outframe.Marshal(netout)
	}
}
