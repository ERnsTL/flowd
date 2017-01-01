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
		fmt.Println("Usage:", os.Args[0], "[host]:[port]")
		os.Exit(1)
	}
	if termutil.Isatty(os.Stdin.Fd()) {
		fmt.Println("ERROR: nothing piped on STDIN")
	} else {
		fmt.Println("ok, found something piped on STDIN")

		fmt.Println("resolving addresses")
		serverAddr, err := net.ResolveUDPAddr("udp", os.Args[1])
		CheckError(err)

		localAddr, err := net.ResolveUDPAddr("udp", "0.0.0.0:0")
		CheckError(err)

		fmt.Println("connecting")
		conn, err := net.DialUDP("udp4", localAddr, serverAddr)
		CheckError(err)
		defer conn.Close()
		fmt.Println("connected")

		fmt.Println("sending")
		// copy STDIN to UDP connection
		io.Copy(conn, os.Stdin)
		fmt.Println("done")
	}
}

func CheckError(err error) {
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
}
