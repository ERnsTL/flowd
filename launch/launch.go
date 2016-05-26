package main

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"net/url"
	"os"
	"os/exec"
	"strconv"
	"time"
)

// type for component connection endpoint definition
type endpoint struct {
	Url        *url.URL
	Addr       net.UDPAddr
	Conn       *net.UDPConn
	listenPort string
}

// implement flag.Value interface
func (e *endpoint) String() string {
	//return fmt.Sprint(*e)
	return fmt.Sprintf("%s://%s", e.Url.Scheme, e.Url.Host)
}

func (e *endpoint) Set(value string) error {
	// If we wanted to allow the flag to be set multiple times,
	// accumulating values, we would delete this if statement.
	// That would permit usages such as
	//	-deltaT 10s -deltaT 15s
	// and other combinations.
	/*
		if e.Scheme != "" {
			return errors.New("interval flag already set")
		}
	*/
	url, err := url.Parse(value)
	if err != nil {
		return errors.New("could not parse flag value: " + err.Error())
	}
	// convert just-parsed URL to endpoint and replace *this* endpoint
	//*e = *(*endpoint)(e2)
	// filter unallowed URL parts; only scheme://host:port is allowed
	// NOTE: url.Opaque is eg. localhost:0 -> always present
	if url.User != nil {
		return errors.New("unallowed URL form: user part not nil")
	}
	if url.Path != "" {
		return errors.New("unallowed URL form: path part not nil")
	}
	if url.RawPath != "" {
		return errors.New("unallowed URL form: raw path part not nil")
	}
	if url.RawQuery != "" {
		return errors.New("unallowed URL form: raw query part not nil")
	}
	if url.Fragment != "" {
		return errors.New("unallowed URL form: fragment part not nil")
	}
	// check for required URL parts
	if url.Scheme == "" {
		return errors.New("unallowed URL form: scheme missing")
	}
	if url.Scheme != "udp" && url.Scheme != "udp4" && url.Scheme != "udp6" {
		return errors.New("unallowed URL form: unimplemented scheme: only {udp,udp4,udp6} allowed")
	}
	if url.Host == "" {
		return errors.New("unallowed URL form: missing host:port or //")
	} else {
		_, portStr, err := net.SplitHostPort(url.Host)
		if err != nil {
			return errors.New("unallowed URL form: host and/or port unvalid: " + err.Error())
		}
		var port int
		if portStr == "" {
			port = 0
		} else {
			port, err = strconv.Atoi(portStr)
			if err != nil {
				return errors.New("unallowed URL form: port malformed, only numbers allowed: " + err.Error())
			}
			if port < 0 || port > 65535 {
				return errors.New("unallowed URL form: port out of range: allowed range is [0;65535]")
			}
		}
		// TODO save the int port and addr somewhere inside ourselves to avoid duplicate work
	}
	// looks ok, save it
	e.Url = url
	return nil
}

func (e *endpoint) Dial() {
	// NOTE: net.ParseIP is not enough, returns nil for textual address -> resolve
	oaddr, err := net.ResolveUDPAddr("udp4", e.Url.Host)
	if err != nil {
		fmt.Println("ERROR: resolving output endpoint address for initial connection:", err)
	}
	oconn, err := net.DialUDP("udp4", &net.UDPAddr{IP: net.ParseIP("127.0.0.1"), Port: 0}, oaddr)
	if err != nil {
		fmt.Println("ERROR: could not dial UDP output connection:", err)
		os.Exit(3)
	}
	e.Conn = oconn
}

func (e *endpoint) Listen() {
	// NOTE: net.ParseIP is not enough, returns nil for textual address -> resolve
	iaddr, err := net.ResolveUDPAddr("udp4", e.Url.Host)
	if err != nil {
		fmt.Println("ERROR could not resolve in endpoint address:", err)
		os.Exit(2)
	}
	conn, err := net.ListenUDP("udp4", iaddr)
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(4)
	}
	_, actualPort, _ := net.SplitHostPort(conn.LocalAddr().String())
	//actualPort := strconv.Itoa(port)
	e.listenPort = actualPort
	e.Conn = conn
}

func main() {
	// read program arguments
	inEndpoint := endpoint{Url: &url.URL{Scheme: "udp", Host: "localhost:0"}}
	outEndpoint := endpoint{Url: &url.URL{Scheme: "udp", Host: "localhost:0"}}
	var help bool
	flag.Var(&inEndpoint, "in", "input endpoint in URL format, ie. udp://localhost:0")
	flag.Var(&outEndpoint, "out", "input endpoint in URL format, ie. udp://localhost:0")
	flag.BoolVar(&help, "h", false, "print usage information")
	flag.Parse()
	if flag.NArg() == 0 {
		fmt.Println("ERROR: missing command to run")
		printUsage()
	}
	if help {
		printUsage()
	}

	// connect to next component in pipeline
	outEndpoint.Dial()
	// listen for input from other components
	inEndpoint.Listen()
	// close connections later
	defer inEndpoint.Conn.Close()
	defer outEndpoint.Conn.Close()

	// start subprocess with arguments
	cmd := exec.Command(flag.Arg(0), flag.Args()[1:]...)
	cout, err := cmd.StdoutPipe()
	if err != nil {
		fmt.Println("ERROR: could not allocate pipe from component stdout:", err)
	}
	cin, err := cmd.StdinPipe()
	if err != nil {
		fmt.Println("ERROR: could not allocate pipe to component stdin:", err)
	}
	cmd.Stderr = os.Stderr
	if err := cmd.Start(); err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	defer cout.Close()
	defer cin.Close()

	// transfer data between socket and component stdin/out
	// NOTE: io.Copy copies from right argument to left
	go func() {
		// input network endpoint -> component stdin
		if _, err := io.Copy(cin, inEndpoint.Conn); err != nil {
			fmt.Println("ERROR: receiving from network input:", err, "Closing.")
			inEndpoint.Conn.Close()
			return
		}
		/*
			NOTE: this using manual buffering, though more debug output
				buf := make([]byte, 1024)
				for {
					n, addr, err := conn.ReadFromUDP(buf)
					//fmt.Println("NET-IN received", n, "bytes from ", addr) //" with contents:", string(buf[0:n]))
					if err != nil {
						//fmt.Println(reflect.TypeOf(err))
						if err == io.EOF {
							fmt.Println("EOF from network input. Closing.")
							return
						}
						fmt.Println("ERROR: receiving from network input:", err, "Closing.")
						conn.Close()
						return
					}
					cin.Write(buf[0:n]) // NOTE: simply sending in whole buf would make JSON decoder error because of \x00 bytes beyond payload
					//fmt.Println("STDIN wrote", n, "bytes to component stdin")
					fmt.Println("in xfer", n, "bytes from", addr)
				}
		*/
	}()
	go func() {
		// component stdout -> output network endpoint
		if bytes, err := io.Copy(outEndpoint.Conn, cout); err != nil {
			fmt.Println("ERROR: writing to network output:", err, "Closing.")
			outEndpoint.Conn.Close()
			return
		} else {
			fmt.Println("net output reached EOF. copied", bytes, "bytes from component stdout -> connection")
		}
		/*
			NOTE this using manual buffering
			buf := make([]byte, 1024)
			for {
				n, err := cout.Read(buf)
				if err != nil {
					if err == io.EOF {
						fmt.Println("EOF from component stdout. Closing.")
					} else {
						fmt.Println("ERROR reading from component stdout:", err, "- closing.")
					}
					return
				} else {
					//fmt.Println("STDOUT received", n, "bytes from component stdout") //string(buf[0:n])
				}
				oconn.Write(buf[0:n]) // NOTE: only write slice of buffer containing actual data
				//fmt.Println("NET-OUT wrote", n, "bytes to next component over network")
				fmt.Println("out xfer", n, "bytes")
			}
		*/
	}()

	// trigger on signal (SIGHUP, SIGUSR1, SIGUSR2, etc.) to reconnect, reconfigure etc.
	//TODO

	// declare network ports
	//TODO

	// make discoverable
	pub := exec.Command("avahi-publish-service", "--subtype", "_web._sub._flowd._udp", "some component", "_flowd._udp", inEndpoint.listenPort, "sometag=true")
	if err := pub.Start(); err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(4)
	}
	defer pub.Process.Kill()

	// return port number
	fmt.Println(inEndpoint.listenPort)

	// post success
	//TODO subprocess logger
	//TODO logger -t flowd -p daemon.info/error/crit/emerg "Starting up"

	cmd.Wait()
	// send out any remaining output from component stdout
	time.Sleep(2 * time.Second)
}

func printUsage() {
	fmt.Println("Usage:", os.Args[0], "-in [input-endpoint]", "-out [output-endpoint]", "[component-cmd]", "[component-args...]")
	flag.PrintDefaults()
	os.Exit(1)
}
