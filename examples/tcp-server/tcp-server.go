package main

import (
	"fmt"
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

	// handle responses from FBP network = STDIN to TCP sockets
	go func() {
		// TODO check if there is something piped on STDIN
		//TODO implement
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
