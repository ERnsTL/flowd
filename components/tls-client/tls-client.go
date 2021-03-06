package main

import (
	"bufio"
	"crypto/tls"
	"flag"
	"fmt"
	"io"
	"net"
	"net/url"
	"os"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
	"github.com/signalsciences/tlstext"
)

const bufSize = 65536

func main() {
	// get configuration from arguments = Unix IIP
	var retry, bridge bool
	unixfbp.DefFlags()
	flag.BoolVar(&bridge, "bridge", true, "bridge mode, true = forward frames from/to FBP network, false = send frame body over TLS, frame data from TLS")
	flag.BoolVar(&retry, "retry", false, "retry connection and try to reconnect")
	flag.Parse()
	if flag.NArg() != 1 {
		fmt.Fprintln(os.Stderr, "ERROR: missing remote address - exiting.")
		printUsage()
		flag.PrintDefaults()
		os.Exit(2)
	}
	// connect to FBP network
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

	//TODO implement
	if retry {
		fmt.Fprintln(os.Stderr, "ERROR: flag -retry currently unimplemented - exiting.")
		os.Exit(2)
	}

	// parse remote address as URL
	// NOTE: add double slashes after semicolon so that host:port is put into .Host
	remoteURL, err := url.ParseRequestURI(flag.Args()[0])
	checkError(err)
	remoteNetwork := remoteURL.Scheme
	//fmt.Fprintf(os.Stderr, "Scheme=%s, Opaque=%s, Host=%s, Path=%s\n", listenURL.Scheme, listenURL.Opaque, listenURL.Host, listenURL.Path)
	remoteHost := remoteURL.Host // includes :[port] if present

	// set up TLS connection parameters
	tlsConfig := &tls.Config{
		CipherSuites: []uint16{
			tls.TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305,
			tls.TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
			tls.TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305,
			tls.TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
		},
		//CurvePreferences: []tls.CurveID{...}
		PreferServerCipherSuites: false, // server selects from client preferences
		MinVersion:               tls.VersionTLS12,
		InsecureSkipVerify:       false,
	}

	//TODO add flag to give client certificate
	//TODO add flag to give private CA certificate (instead of system CA certificates)
	//TODO add mutual auth: http://www.bite-code.com/2015/06/25/tls-mutual-auth-in-golang/
	// if -ca
	/*
		cacerts := x509.NewCertPool()
		pemData, err := ioutil.ReadFile(pemPath)
		if err != nil {
			// do error
		}
		cacerts.AppendCertsFromPEM(pemData)
		tlsConfig.RootCAs = cacerts
	*/

	//TODO refactor to actually be able to retry connections and try to reconnect
	//TODO maybe useful for resuming: https://golang.org/pkg/crypto/tls/#ClientSessionCache
	fmt.Fprintln(os.Stderr, "connecting...")
	conn, err := tls.Dial(remoteNetwork, remoteHost, tlsConfig)
	checkError(err)
	defer conn.Close()
	fmt.Fprintln(os.Stderr, "connected")
	//TODO probably better to write own short function for the few cases above, which outputs directly in the desired format
	fmt.Fprintln(os.Stderr, strings.ToLower(tlstext.Version(conn.ConnectionState().Version)), "using", strings.TrimPrefix(strings.Replace(strings.ToLower(tlstext.CipherSuite(conn.ConnectionState().CipherSuite)), "_", " ", -1), "tls "))
	//TODO notify downstream components using OpenNotification, see tcp-server

	remoteAddress := strings.SplitN(remoteHost, ":", 2)[0]
	err = conn.VerifyHostname(remoteAddress)
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: certificate chain unvalid:", err.Error())
		os.Exit(2)
	}
	fmt.Fprintln(os.Stderr, "certificate chain valid")

	//TODO check that the server name in the cert matches, not just that the certificate chain is OK.

	if bridge {
		fmt.Fprintln(os.Stderr, "forwarding frames")
		// copy STDIN to network connection
		go func() {
			_, err := io.Copy(conn, os.Stdin)
			if err != nil {
				fmt.Fprintln(os.Stderr, "ERROR on FBP->TLS:", err)
				return
			}
			fmt.Fprintln(os.Stderr, "FBP->TLS closed")
		}()

		// copy network connection to STDOUT, blocking until EOF
		if _, err := io.Copy(os.Stdout, conn); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR on TLS->FBP:", err)
			return
		}
		fmt.Fprintln(os.Stderr, "TLS->FBP closed")
	} else {
		// send and receive frame bodies
		closeChan := make(chan bool)

		// handle TLS -> FBP
		go handleConnection(conn, closeChan, netout)

		// handle FBP -> TLS
		//TODO pretty much 1:1 copy of tcp-server main loop
		go func() {
			for {
				frame, err := flowd.Deserialize(netin)
				if err != nil {
					if err == io.EOF {
						fmt.Fprintln(os.Stderr, "tls out: EOF from FBP network. Exiting.")
					} else {
						fmt.Fprintln(os.Stderr, "tls out: ERROR parsing frame from FBP network:", err, "- Exiting.")
						//TODO notification feedback into FBP network
					}
					os.Stdin.Close()
					//TODO gracefully shut down / close all connections
					os.Exit(3)
					return
				}

				if unixfbp.Debug {
					fmt.Fprintln(os.Stderr, "tls out: received frame type", frame.Type, "data type", frame.BodyType, "for port", frame.Port, "with body:", string(frame.Body))
				} else if !unixfbp.Quiet {
					fmt.Fprintln(os.Stderr, "tls out: frame in with", len(frame.Body), "bytes body")
				}

				//TODO check for non-data/control frames
				//FIXME send close notification downstream also in error cases (we close conn) or if client shuts down connection (EOF)
				//TODO error feedback for unknown/unconnected/closed TLS connections

				// write frame body out to TLS connection
				if bytesWritten, err := conn.Write(frame.Body); err != nil {
					//TODO check for EOF
					fmt.Fprintf(os.Stderr, "tls out: ERROR writing to TLS connection with %s: %s - closing.\n", conn.RemoteAddr(), err)
					//TODO gracefully shut down / close all connections
					os.Exit(1)
				} else if bytesWritten < len(frame.Body) {
					// short write
					fmt.Fprintf(os.Stderr, "tls out: ERROR: short send to TLS connection with %s, only %d of %d bytes written - closing.\n", conn.RemoteAddr(), bytesWritten, len(frame.Body))
					//TODO gracefully shut down / close all connections
					os.Exit(1)
				} else {
					// success
					//TODO if !quiet - add that flag
					fmt.Fprintf(os.Stderr, "tls out: wrote %d bytes to %s\n", bytesWritten, conn.RemoteAddr())
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
	fmt.Fprintln(os.Stderr, "Arguments: [-debug] [-quiet] [-bridge] [-retry] [host]:[port]")
}

//TODO optimize: give netout as parameter or use global variable? safe for concurrent use?
//TODO pretty much 1:1 copy from tcp-server handleConnection() -> reuse?
func handleConnection(conn *tls.Conn, closeChan chan<- bool, netout *bufio.Writer) {
	// prepare data structures
	buf := make([]byte, bufSize)
	outframe := flowd.Frame{
		Type:     "data",
		BodyType: "TLSPacket",
		//Port:     "OUT",
		//ContentType: "application/octet-stream",
		Extensions: nil,
		Body:       nil,
	}

	// process TCP-inside-TLS packets
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
				fmt.Fprintln(os.Stderr, "tls in: EOF on connection, closing.")
			} else if neterr, isneterr := err.(net.Error); isneterr && neterr.Timeout() {
				// network timeout
				fmt.Fprintf(os.Stderr, "tls in: ERROR reading from %v: timeout: %s, closing.\n", conn.RemoteAddr(), neterr)
			} else {
				// other error
				fmt.Fprintf(os.Stderr, "tls in: ERROR: %s - closing.\n", err)
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

		if unixfbp.Debug {
			fmt.Fprintf(os.Stderr, "tls in: read %d bytes from %s: %s\n", bytesRead, conn.RemoteAddr(), buf[:bytesRead])
		} else if !unixfbp.Quiet {
			fmt.Fprintf(os.Stderr, "tls in: read %d bytes from %s\n", bytesRead, conn.RemoteAddr())
		}

		// frame TLS packet into flowd frame
		outframe.Body = buf[:bytesRead]

		// send it to STDOUT = FBP network
		outframe.Serialize(netout)
		// send it now (flush buffer)
		if err = netout.Flush(); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
		}
	}
}
