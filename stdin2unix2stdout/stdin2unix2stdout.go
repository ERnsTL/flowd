package main

import (
	"fmt"
	"io"
	"net"
	"os"

	termutil "github.com/andrew-d/go-termutil"
)

func main() {
	if len(os.Args) < 1+1 {
		fmt.Fprintln(os.Stderr, "Usage:", os.Args[0], "[@][server-pathname] [@][optional-back-channel-path]")
		fmt.Fprintln(os.Stderr, "If one address is given, then bi-directional operation. If two given, then 2x uni-directional.")
		os.Exit(1)
	}
	if termutil.Isatty(os.Stdin.Fd()) {
		fmt.Fprintln(os.Stderr, "ERROR: nothing piped on STDIN")
	} else {
		fmt.Fprintln(os.Stderr, "ok, found something piped on STDIN")

		if len(os.Args) == 1+2 {
			// two unidirectional connections
			fmt.Fprintln(os.Stderr, "starting 2x uni-directonal copy...")
			done := make(chan bool, 1)
			up := make(chan bool, 2)

			// connecting socket going into FBP network netin
			go func() {
				fmt.Fprintln(os.Stderr, "resolving network NETIN address...")
				serverAddr, err := net.ResolveUnixAddr("unixpacket", os.Args[1])
				checkError(err)

				fmt.Fprintln(os.Stderr, "connecting to FBP network...")
				conn, err := net.DialUnix("unixpacket", nil, serverAddr)
				checkError(err)
				defer conn.Close()
				fmt.Fprintln(os.Stderr, "connected to FBP network.")
				// copy STDIN to network connection
				up <- true
				io.Copy(conn, os.Stdin)
				done <- true
			}()

			// listen socket for network out
			go func() {
				fmt.Fprintln(os.Stderr, "resolving listen address...")
				listenAddr, err := net.ResolveUnixAddr("unixpacket", os.Args[2])
				checkError(err)

				fmt.Fprintln(os.Stderr, "opening listen socket...")
				listener, err := net.ListenUnix("unixpacket", listenAddr)
				checkError(err)

				fmt.Fprintln(os.Stderr, "listening...")
				listenConn, _ := listener.AcceptUnix()
				fmt.Fprintln(os.Stderr, "accepted connection from FBP network.")
				defer listenConn.Close()

				// copy network connection to STDOUT
				up <- true
				io.Copy(os.Stdout, listenConn)

				fmt.Fprintln(os.Stderr, "closing listener...")
				listener.Close()

				done <- true
			}()

			// wait for bridge up
			<-up
			<-up
			fmt.Fprintln(os.Stderr, "bridge up.")

			// wait for connection close or error
			<-done
		} else {
			fmt.Fprintln(os.Stderr, "resolving address...")
			serverAddr, err := net.ResolveUnixAddr("unixpacket", os.Args[1])
			checkError(err)

			fmt.Fprintln(os.Stderr, "connecting...")
			conn, err := net.DialUnix("unixpacket", nil, serverAddr)
			checkError(err)
			defer conn.Close()

			// set up bi-directional copy
			fmt.Fprintln(os.Stderr, "starting bi-directional copy...")
			done := make(chan bool, 1)
			// copy STDIN to network connection
			go func() {
				io.Copy(conn, os.Stdin)
				done <- true
			}()
			// copy network connection to STDOUT
			go func() {
				io.Copy(os.Stdout, conn)
				done <- true
			}()

			// wait for connection close or error
			fmt.Fprintln(os.Stderr, "bridge up.")
			<-done
		}
	}
}

func checkError(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR:", err)
		os.Exit(2)
	}
}
