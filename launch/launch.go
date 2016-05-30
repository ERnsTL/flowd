package main

import (
	"bytes"
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

	"github.com/ERnsTL/flowd/libflowd"
)

const bufSize = 65535

// type for component connection endpoint definition
type endpoint struct {
	Url        *url.URL
	Addr       net.UDPAddr
	Conn       *net.UDPConn
	listenPort string
}

type inputEndpoint endpoint
type outputEndpoint endpoint

func (e *outputEndpoint) Dial() {
	// NOTE: net.ParseIP is not enough, returns nil for textual address -> resolve
	oaddr, err := net.ResolveUDPAddr("udp4", e.Url.Host)
	if err != nil {
		fmt.Println("ERROR: resolving output endpoint address for initial connection:", err)
	}
	//TODO make protocol-agnostic using net.DialAddr()
	oconn, err := net.DialUDP("udp4", &net.UDPAddr{IP: net.ParseIP("127.0.0.1"), Port: 0}, oaddr)
	if err != nil {
		fmt.Println("ERROR: could not dial UDP output connection:", err)
		os.Exit(3)
	}
	e.Conn = oconn
}

func (e *inputEndpoint) Listen() {
	// NOTE: net.ParseIP is not enough, returns nil for textual address -> resolve
	iaddr, err := net.ResolveUDPAddr("udp4", e.Url.Host)
	if err != nil {
		fmt.Println("ERROR: could not resolve in endpoint address:", err)
		os.Exit(2)
	}
	conn, err := net.ListenUDP("udp4", iaddr)
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(4)
	}
	_, actualPort, _ := net.SplitHostPort(conn.LocalAddr().String())
	//actualPort := strconv.Itoa(port)
	//TODO decide whether to keep string or int representation
	e.listenPort = actualPort
	e.Conn = conn
}

//TODO seems useless, because Conn.Close can be called directly
func (e *inputEndpoint) Close() {
	e.Conn.Close()
}
func (e *outputEndpoint) Close() {
	e.Conn.Close()
}

// types to hold information on a collection of endpoints, ie. all input endpoints
// NOTE: using map of pointers, because map elements are not addressable
type inputEndpoints map[string]*inputEndpoint
type outputEndpoints map[string]*outputEndpoint

// implement flag.Value interface
func (e *inputEndpoints) String() string {
	for _, endpoint := range *e {
		return fmt.Sprintf("%s://%s#%s", endpoint.Url.Scheme, endpoint.Url.Host, endpoint.Url.Fragment)
	}
	return ""
}
func (e *outputEndpoints) String() string {
	for _, endpoint := range *e {
		return fmt.Sprintf("%s://%s#%s", endpoint.Url.Scheme, endpoint.Url.Host, endpoint.Url.Fragment)
	}
	return ""
}

// NOTE: can be called multiple times if there are multiple occurrences of the -in resp. -out flags
// NOTE: if only one occurrence shall be allowed, check if a required property is already set
func (e *inputEndpoints) Set(value string) error {
	if parsedUrl, err := parseEndpointURL(value); err != nil {
		return err
	} else {
		(*e)[parsedUrl.Fragment] = &inputEndpoint{Url: parsedUrl}
	}
	return nil
}
func (e *outputEndpoints) Set(value string) error {
	if parsedUrl, err := parseEndpointURL(value); err != nil {
		return err
	} else {
		(*e)[parsedUrl.Fragment] = &outputEndpoint{Url: parsedUrl}
	}
	return nil
}

func parseEndpointURL(value string) (url *url.URL, err error) {
	url, err = url.Parse(value)
	if err != nil {
		return nil, errors.New("could not parse flag value: " + err.Error())
	}
	// convert just-parsed URL to endpoint and replace *this* endpoint
	//*e = *(*endpoint)(e2)
	// filter unallowed URL parts; only scheme://host:port is allowed
	// NOTE: url.Opaque is eg. localhost:0 -> usually present, but can be left out to mean 0.0.0.0
	/*
		if url.Opaque == "" {
			return nil, errors.New("unallowed URL form: opaque part = host+port nil")
		}
	*/
	if url.User != nil {
		return nil, errors.New("unallowed URL form: user part not nil")
	}
	if url.Path != "" {
		return nil, errors.New("unallowed URL form: path part not nil")
	}
	if url.RawPath != "" {
		return nil, errors.New("unallowed URL form: raw path part not nil")
	}
	if url.RawQuery != "" {
		return nil, errors.New("unallowed URL form: raw query part not nil")
	}
	if url.Fragment == "" {
		return nil, errors.New("unallowed URL form: fragment nil, must be name of port")
	}
	// check for required URL parts
	if url.Scheme == "" {
		return nil, errors.New("unallowed URL form: scheme missing")
	}
	if url.Scheme != "udp" && url.Scheme != "udp4" && url.Scheme != "udp6" {
		return nil, errors.New("unallowed URL form: unimplemented scheme: only {udp,udp4,udp6} allowed")
	}
	if url.Host == "" {
		return nil, errors.New("unallowed URL form: missing host:port or //")
	} else {
		_, portStr, err := net.SplitHostPort(url.Host)
		if err != nil {
			return nil, errors.New("unallowed URL form: host and/or port unvalid: " + err.Error())
		}
		var port int
		if portStr == "" {
			port = 0
		} else {
			port, err = strconv.Atoi(portStr)
			if err != nil {
				return nil, errors.New("unallowed URL form: port malformed, only numbers allowed: " + err.Error())
			}
			if port < 0 || port > 65535 {
				return nil, errors.New("unallowed URL form: port out of range: allowed range is [0;65535]")
			}
		}
		// TODO save the int port and addr somewhere inside ourselves to avoid duplicate work
	}
	return url, nil
}

func (e *inputEndpoints) Listen() {
	for name, ep := range *e {
		fmt.Println("connecting input", name)
		ep.Listen()
	}
}
func (e *outputEndpoints) Dial() {
	for name, ep := range *e {
		fmt.Println("connecting output", name)
		ep.Dial()
	}
}
func (e *inputEndpoints) Close() {
	for _, ep := range *e {
		ep.Close()
	}
}
func (e *outputEndpoints) Close() {
	for _, ep := range *e {
		ep.Close()
	}
}

func main() {
	// read program arguments
	inEndpoints := inputEndpoints{}
	outEndpoints := outputEndpoints{}
	var help, debug, quiet bool
	var inFraming, outFraming bool
	flag.Var(&inEndpoints, "in", "input endpoint(s) in URL format, ie. udp://localhost:0#portname")
	flag.Var(&outEndpoints, "out", "output endpoint(s) in URL format, ie. udp://localhost:0#portname")
	flag.BoolVar(&inFraming, "inframing", true, "perform frame decoding and routing on input endpoints")
	flag.BoolVar(&outFraming, "outframing", true, "perform frame decoding and routing on output endpoints")
	flag.BoolVar(&help, "h", false, "print usage information")
	flag.BoolVar(&debug, "debug", false, "give detailed event output")
	flag.BoolVar(&quiet, "quiet", false, "no informational output except errors")
	flag.Parse()
	if flag.NArg() == 0 {
		fmt.Println("ERROR: missing command to run")
		printUsage()
	}
	if help {
		printUsage()
	}

	// connect with other components
	outEndpoints.Dial()
	inEndpoints.Listen()
	defer inEndpoints.Close()
	defer outEndpoints.Close()

	// start component as subprocess, with arguments
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

	// input = transfer input network endpoint to component stdin
	if debug {
		fmt.Println("input framing set to", inFraming)
	}
	if !inFraming {
		// NOTE: using Go stdlib without any processing
		// NOTE: io.Copy copies from right argument to left
		for inEndpoint := range inEndpoints {
			go func(ep *inputEndpoint) {
				if _, err := io.Copy(cin, ep.Conn); err != nil {
					fmt.Println("ERROR: receiving from network input:", err, "Closing.")
					ep.Conn.Close()
					return
				}
			}(inEndpoints[inEndpoint])
		}
	} else {
		// NOTE: using manual buffering, though more debug output and we can do framing
		for inEndpoint := range inEndpoints {
			go func(ep *inputEndpoint) {
				buf := make([]byte, bufSize)
				var nPrev int
				for {
					// read from connection
					n, addr, err := ep.Conn.ReadFromUDP(buf[nPrev:])
					// NOTE: above can lead to reading only parts of an UDP packet even if buffer is large enough
					if err != nil {
						if err == io.EOF {
							fmt.Println("EOF from network input. Closing.")
						} else {
							fmt.Println("ERROR: receiving from network input:", err, "Closing.")
						}
						ep.Conn.Close()
						return
					}
					if debug {
						fmt.Println("net in received", n, "bytes from", addr) //" with contents:", string(buf[0:n]))
					}
					// decode frame
					bufReader := bytes.NewReader(buf[0 : n+nPrev])
					//flowd.ParseFrame(ep.Conn)
					if fr, err := flowd.ParseFrame(bufReader); err != nil {
						// failed to parse, try re-assembly using max. 2 fragments, buf[0:nPrev-1] and buf[nPrev:nPrev+n]
						// NOTE: an io.Scanner with a ScanFunc could also be used, but this is simpler
						if nPrev > 0 {
							// already tried once, discard previous fragment
							fmt.Println("net in: ERROR parsing frame, discarding buffer:", err.Error())
							nPrev = 0
						} else {
							// try again later using more arrived frames
							if debug {
								fmt.Println("net in: WARNING uncomplete/malformed frame, trying re-assembly:", err.Error())
							} else if !quiet {
								fmt.Println("net in: WARNING uncomplete/malformed frame, trying re-assembly")
							}
							nPrev = n
						}
					} else { // parsed fine
						if debug {
							fmt.Println("received frame type", fr.Type, "data type", fr.BodyType, "for port", fr.Port, "with body:", (string)(*fr.Body))
						}

						//TODO check frame's Port header if an input port of that name exists in inEndpoints

						// forward frame to component
						cin.Write(buf[0 : n+nPrev]) // NOTE: simply sending in whole buf would make body JSON decoder error because of \x00 bytes beyond payload
						if debug {
							fmt.Println("STDIN wrote", nPrev+n, "bytes to component stdin")
						}
						if !quiet {
							fmt.Println("in xfer", nPrev+n, "bytes from", addr)
						}

						// reset previous packet size
						nPrev = 0
					}
				}
			}(inEndpoints[inEndpoint])
		}
	}

	// output = transfer component stdout to output network endpoint
	if debug {
		fmt.Println("output framing set to", outFraming)
	}
	if !outFraming {
		// NOTE: using Go stdlib, without processing
		for outEndpoint := range outEndpoints {
			go func(ep *outputEndpoint) {
				if bytes, err := io.Copy(ep.Conn, cout); err != nil {
					fmt.Println("ERROR: writing to network output:", err, "Closing.")
					ep.Conn.Close()
					return
				} else {
					fmt.Println("net output reached EOF. copied", bytes, "bytes from component stdout -> connection")
				}
			}(outEndpoints[outEndpoint])
		}
	} else {
		// NOTE: this using manual buffering
		go func() {
			buf := make([]byte, bufSize)
			var nPrev int
			for {
				n, err := cout.Read(buf[nPrev:])
				if err != nil {
					if err == io.EOF {
						fmt.Println("EOF from component stdout. Closing.")
					} else {
						fmt.Println("ERROR reading from component stdout:", err, "- closing.")
					}
					outEndpoints.Close()
					return
				} else {
					if debug {
						fmt.Println("STDOUT received", n, "bytes from component stdout")
					}
				}
				if frame, err := flowd.ParseFrame(bytes.NewReader(buf[0 : nPrev+n])); err != nil { //TODO optimize, NewReader should not be neccessary
					// frame uncomplete, but not malformed -> wait for remaining fragments
					if debug {
						fmt.Println("WARNING: uncomplete frame from component stdout, re-assembling:", err.Error())
					}
					nPrev = nPrev + n
				} else { // frame complete now
					if debug {
						fmt.Println("OK parsing frame from component stdout")
					}

					// write out to network
					//TODO error feedback for unknown output ports
					if e, exists := outEndpoints[frame.Port]; exists {
						// NOTE: only write slice of buffer containing actual data
						if _, err := e.Conn.Write(buf[0 : nPrev+n]); err != nil {
							fmt.Println("NET-OUT ERROR sending to output endpoint:", err.Error(), "- closing.")
							outEndpoints[frame.Port].Close()
							//TODO return as well = close down all output operations or allow one output to fail?
						} else {
							//TODO having two outputs say the same seems useless
							if debug {
								fmt.Println("NET-OUT wrote", nPrev+n, "bytes to port", frame.Port, "over network")
							}
							if !quiet {
								fmt.Println("out xfer", nPrev+n, "bytes to", frame.Port)
							}
						}
					} else {
						fmt.Println("NET-OUT ERROR: component tried sending to undeclared port. Exiting.")
						outEndpoints.Close()
						return
					}
					// reset size of any previous fragment(s)
					nPrev = 0
				}
			}
		}()
	}

	// trigger on signal (SIGHUP, SIGUSR1, SIGUSR2, etc.) to reconnect, reconfigure etc.
	//TODO

	// declare network ports
	//TODO

	// make discoverable
	//TODO publish all input ports
	pub := exec.Command("avahi-publish-service", "--service", "--subtype", "_web._sub._flowd._udp", "some component", "_flowd._udp", inEndpoints["in"].listenPort, "sometag=true")
	if err := pub.Start(); err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(4)
	}
	defer pub.Process.Kill()

	// return port number
	fmt.Println(inEndpoints["in"].listenPort) //FIXME port "in" may not exist

	// post success
	//TODO subprocess logger
	//TODO logger -t flowd -p daemon.info/error/crit/emerg "Starting up"

	cmd.Wait()
	// send out any remaining output from component stdout
	time.Sleep(2 * time.Second)
}

func printUsage() {
	fmt.Println("Usage:", os.Args[0], "-in [input-endpoint(s)]", "-out [output-endpoint(s)]", "[component-cmd]", "[component-args...]")
	flag.PrintDefaults()
	os.Exit(1)
}
