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
	debug, quiet bool
	netout       *bufio.Writer //TODO is that safe for concurrent use in this case here? restructure handleConnection!
)

func main() {
	// open connection to FBP network
	netin := bufio.NewReader(os.Stdin)
	netout = bufio.NewWriter(os.Stdout)
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	iip, err := flowd.GetIIP("CONF", netin)
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- exiting.")
		os.Exit(1)
	}
	// parse IIP
	flags := flag.NewFlagSet("unix-client", flag.ContinueOnError)
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
	// NOTE: no double slashes after semicolon, otherwise what is given after that
	// gets put into .Host and .Path and @ (for abstract sockets) cannot be recognized
	remoteURL, err := url.ParseRequestURI(flags.Args()[0])
	checkError(err)
	remoteNetwork := remoteURL.Scheme
	//fmt.Fprintf(os.Stderr, "Scheme=%s, Opaque=%s, Host=%s, Path=%s\n", listenURL.Scheme, listenURL.Opaque, listenURL.Host, listenURL.Path)
	remotePath := remoteURL.Opaque
	if remoteNetwork == "unixgram" {
		fmt.Fprintln(os.Stderr, "ERROR: network 'unixgram' unimplemented, refer to unixgram-client component - Exiting.") //TODO implement that
		os.Exit(1)
	}

	fmt.Fprintln(os.Stderr, "resolving address")
	remoteAddr, err := net.ResolveUnixAddr(remoteNetwork, remotePath)
	checkError(err)

	fmt.Fprintln(os.Stderr, "connecting...")
	//TODO add flag to set local address/name
	conn, err := net.DialUnix(remoteAddr.Network(), nil, remoteAddr)
	checkError(err)
	defer conn.Close()
	fmt.Fprintln(os.Stderr, "connected")

	done := make(chan bool)
	up := make(chan bool)
	if bridge {
		// set up bi-directional copy
		fmt.Fprintln(os.Stderr, "starting bridge...")
		// copy STDIN to network connection
		go func() {
			up <- true
			io.Copy(conn, netin)
			done <- true
		}()
		// copy network connection to STDOUT
		go func() {
			up <- true
			io.Copy(netout, conn)
			done <- true
		}()

		// wait for bridge up
		<-up
		<-up
		fmt.Fprintln(os.Stderr, "bridge up.")

		// wait for connection close or error
		<-done
	} else {

		// handle UNIX socket -> FBP
		go handleConnection(conn, done)

		// handle FBP -> UNIX socket
		//TODO pretty much 1:1 copy of tcp-server main loop
		go func() {
			for {
				frame, err := flowd.ParseFrame(netin)
				if err != nil {
					if err == io.EOF {
						fmt.Fprintln(os.Stderr, "unix out: EOF from FBP network on STDIN. Exiting.")
					} else {
						fmt.Fprintln(os.Stderr, "unix out: ERROR parsing frame from FBP network on STDIN:", err, "- Exiting.")
						//TODO notification feedback into FBP network
					}
					os.Stdin.Close()
					//TODO gracefully shut down / close all connections
					os.Exit(3)
					return
				}

				if debug {
					fmt.Fprintln(os.Stderr, "unix out: received frame type", frame.Type, "data type", frame.BodyType, "for port", frame.Port, "with body:", string(frame.Body))
				} else if !quiet {
					fmt.Fprintln(os.Stderr, "unix out: frame in with", len(frame.Body), "bytes body")
				}

				//TODO check for non-data/control frames
				//FIXME send close notification downstream also in error cases (we close conn) or if client shuts down connection (EOF)
				//TODO error feedback for unknown/unconnected/closed TCP connections

				// write frame body out to UNIX connection
				if bytesWritten, err := conn.Write(frame.Body); err != nil {
					//TODO check for EOF
					fmt.Fprintf(os.Stderr, "unix out: ERROR writing to UNIX connection with %s: %s - closing.\n", conn.RemoteAddr(), err)
					//TODO gracefully shut down / close all connections
					os.Exit(1)
				} else if bytesWritten < len(frame.Body) {
					// short write
					fmt.Fprintf(os.Stderr, "unix out: ERROR: short send to UNIX connection with %s, only %d of %d bytes written - closing.\n", conn.RemoteAddr(), bytesWritten, len(frame.Body))
					//TODO gracefully shut down / close all connections
					os.Exit(1)
				} else {
					// success
					if !quiet {
						fmt.Fprintf(os.Stderr, "unix out: wrote %d bytes to %s\n", bytesWritten, conn.RemoteAddr())
					}
				}

				if frame.BodyType == "CloseConnection" {
					fmt.Fprintln(os.Stderr, "got close command, closing connection.")
					// close command received, close connection
					conn.Close()
					//TODO send into closeChan, dont close it here
				}
			}
		}()

		// wait for connection handlers up
		//TODO make use of these
		//<-up
		//<-up
		fmt.Fprintln(os.Stderr, "connection up.")

		// wait for connection close or error
		<-done
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

//TODO pretty much 1:1 copy from unix-server handleConnection() -> reuse?
func handleConnection(conn *net.UnixConn, closeChan chan bool) {
	// prepare data structures
	buf := make([]byte, bufSize)
	outframe := flowd.Frame{
		Type:       "data",
		BodyType:   "UNIXPacket",
		Port:       "OUT",
		Extensions: nil,
		Body:       nil,
	}

	// process UNIX packets
	for {
		bytesRead, err := conn.Read(buf)
		if err != nil || bytesRead <= 0 {
			// check more specifically
			if err == io.EOF {
				// EOF = closed by peer or already closed @ STDIN handler goroutine or network error
				fmt.Fprintln(os.Stderr, "unix in: EOF on connection, closing.")
			} else if neterr, isneterr := err.(net.Error); isneterr && neterr.Timeout() {
				// network timeout
				fmt.Fprintf(os.Stderr, "unix in: ERROR reading from %v: timeout: %s, closing.\n", conn.RemoteAddr(), neterr)
			} else {
				// other error
				fmt.Fprintf(os.Stderr, "unix in: ERROR: %s - closing.\n", err)
			}
			_ = conn.Close() // NOTE: do not close it here, should be done outside
			// remove conn from list of connections
			closeChan <- true
			// exit
			return
		}

		if debug {
			fmt.Fprintf(os.Stderr, "unix in: read %d bytes from %s: %s\n", bytesRead, conn.RemoteAddr(), buf[:bytesRead])
		} else if !quiet {
			fmt.Fprintf(os.Stderr, "unix in: read %d bytes from %s\n", bytesRead, conn.RemoteAddr())
		}

		// frame UNIX packet into flowd frame
		outframe.Body = buf[:bytesRead]

		// send it to STDOUT = FBP network
		outframe.Marshal(netout)
	}
}
