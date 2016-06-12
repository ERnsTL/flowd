package main

import (
	"bufio"
	"bytes"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"net/url"
	"os"
	"os/exec"
	"regexp"
	"strconv"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
)

const bufSize = 65535

// type for component connection endpoint definition
type endpoint struct {
	Url  *url.URL
	Addr net.Addr
	// TODO unoptimal to have both Conn and Listener on same struct
	Conn       net.Conn
	Listener   net.Listener
	listenPort string
	Ready      chan bool
}

type inputEndpoint endpoint
type outputEndpoint endpoint

func (e *outputEndpoint) Dial() {
	e.Ready = make(chan bool)
	go func() {
		try := 1
	tryagain: //TODO could go into infinite loop. later the orchestrator has to be able to detect these kinds of errors
		oconn, err := net.DialTimeout(e.Url.Scheme, e.Url.Host, 30*time.Second)
		if err != nil {
			nerr, ok := err.(net.Error)
			if ok && try < 20 {
				if try > 10 {
					fmt.Fprintln(os.Stderr, "WARNING: could not dial connection and/or resolve address:", err, "error is permanent?", nerr.Temporary())
				}
				time.Sleep(1 * time.Second)
				goto tryagain
			} else {
				fmt.Fprintln(os.Stderr, "ERROR: could not dial connection and/or resolve address:", err)
			}
			os.Exit(3)
		}
		e.Conn = oconn
		e.Ready <- true
	}()
}

func (e *inputEndpoint) Listen() {
	ilistener, err := net.Listen(e.Url.Scheme, e.Url.Host)
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: listening on ", e.Url.Host, ":", err)
		os.Exit(4)
	}
	_, actualPort, _ := net.SplitHostPort(ilistener.Addr().String())
	//actualPort := strconv.Itoa(port)
	//TODO decide whether to keep string or int representation
	e.listenPort = actualPort
	e.Listener = ilistener

	// accept one connection
	e.Ready = make(chan bool)
	go func(ep *inputEndpoint) {
		// wait for incoming connection
		if iconn, err := ep.Listener.Accept(); err != nil { //TODO accept with timeout -> so that orchestrator can detect something being wrong
			fmt.Fprintln(os.Stderr, "ERROR: accepting connection on", ep.Listener.Addr(), ":")
			os.Exit(4)
		} else {
			ep.Conn = iconn
			ep.Listener.Close() // close listener for further connections
			ep.Ready <- true
		}
	}(e)
}

type NoArgsNoRetFunc func()

func (e *inputEndpoint) ListenAgain(callback NoArgsNoRetFunc) {
	e.Listen()
	<-e.Ready
	callback()
}

//TODO seems useless, because Conn.Close can be called directly
func (e *inputEndpoint) Close() {
	e.Conn.Close()
	// TODO what if listener is not connected yet? close it as well?
	_ = e.Listener.Close()
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
		return nil, errors.New("could not parse flag value as URL: " + err.Error())
	}
	// convert just-parsed URL to endpoint and replace *this* endpoint
	//*e = *(*endpoint)(e2)
	// filter unallowed URL parts; only scheme://host:port is allowed
	// NOTE: url.Opaque is eg. localhost:0 -> usually present, but can be left out to mean 0.0.0.0
	/*
		if url.Opaque == "" {
			return nil, errors.New("opaque part = host+port nil")
		}
	*/
	if url.User != nil {
		if url.User.String() == "" && (url.Scheme == "unix" || url.Scheme == "unixpacket") {
			// OK, this allows for transferring the "@" via the URL for Linux's abstract Unix domain sockets
			url.Host = "@" + url.Host
		} else {
			return nil, errors.New("user part not nil: " + url.User.String())
		}
	}
	if url.Path != "" {
		return nil, errors.New("path part not nil")
	}
	if url.RawPath != "" {
		return nil, errors.New("raw path part not nil")
	}
	if url.RawQuery != "" {
		return nil, errors.New("raw query part not nil")
	}
	if url.Fragment == "" {
		return nil, errors.New("fragment nil, must be name of port")
	} else {
		// check for well-formed port name
		// NOTE: \w is word characters, (?: is grouping without extraction
		re := regexp.MustCompile(`(\w+)(?:\[(\d+)\])?`)
		// NOTE: [0] is match of entire exp, [1] is port name, [2] is array port index
		matches := re.FindStringSubmatch(url.Fragment)
		if matches[0] != url.Fragment {
			return nil, errors.New("port name malformed, must be \\w+ or \\w+[index]")
		} else if len(matches[1]) > 1000 {
			return nil, errors.New("port name too long, maximum allowable is 1000 UTF-8 runes")
		} else if len(matches[2]) > 4 {
			return nil, errors.New("port name array index too long, maximum allowable is 4 digits")
		}
		// TODO save the port name and index somewhere inside of ourselves to avoid duplicate work
	}
	// check for required URL parts
	if url.Scheme == "" {
		return nil, errors.New("scheme missing")
	}
	if matched, err := regexp.MatchString(`^(tcp|tcp4|tcp6|unix|unixpacket)$`, url.Scheme); err != nil {
		return nil, fmt.Errorf("could not parse scheme: %s", err)
	} else if !matched {
		return nil, errors.New("unimplemented scheme: only {tcp,tcp4,tcp6,unix,unixpacket} allowed")
	}
	if url.Host == "" {
		return nil, errors.New("missing host:port or //")
	} else {
		if url.Scheme == "unix" || url.Scheme == "unixpacket" {
			if url.User != nil {
				// OK, this allows for transferring the "@" via the URL for Linux's abstract Unix domain sockets
				url.Host = "@" + url.Host
			}
		} else {
			// TCP, UDP have [host]:[port] format
			_, portStr, err := net.SplitHostPort(url.Host)
			if err != nil {
				return nil, errors.New("host and/or port unvalid: " + err.Error())
			}
			var port int
			if portStr == "" {
				port = 0
			} else {
				port, err = strconv.Atoi(portStr)
				if err != nil {
					return nil, errors.New("port malformed, only numbers allowed: " + err.Error())
				}
				if port < 0 || port > 65535 {
					return nil, errors.New("port out of range: allowed range is [0;65535]")
				}
			}
			// TODO save the int port and addr somewhere inside ourselves to avoid duplicate work
		}
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
	flag.Var(&inEndpoints, "in", "input endpoint(s) in URL format, ie. tcp://localhost:0#portname")
	flag.Var(&outEndpoints, "out", "output endpoint(s) in URL format, ie. tcp://localhost:0#portname")
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

	// return port number
	fmt.Println(inEndpoints["in"].listenPort) //FIXME port "in" may not exist

	// make discoverable so that other components can connect
	//TODO publish all input ports
	pub := exec.Command("avahi-publish-service", "--service", "--subtype", "_web._sub._flowd._tcp", "some component", "_flowd._tcp", inEndpoints["in"].listenPort, "sometag=true")
	if err := pub.Start(); err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(4)
	}
	defer pub.Process.Kill()

	// wait for connections to become ready, otherwise we start the component without all connections set up and it might panic
	//TODO make it possible to see realtime updates when one is connected (1st one may block displaying "Ready" for the others)
	/*
		Select on multiple channels. Make Dial() methods submit to central ready channel (1 for inputs, 1 for outputs) and expect len(inEndpoints+outEndpoints) of ready notifications
		for i := 1; i <= 9; i++ {
		     select {
		     case msg := <-c1:
		          println(msg)
			 case msg := <-c2:
		          println(msg)
		     case msg := <-c3:
		          println(msg)
		     }
		}
	*/
	for name, _ := range inEndpoints {
		ep := inEndpoints[name] //TODO not sure if this is necessary
		if debug {
			fmt.Println("waiting for ready from input", name)
		}
		<-ep.Ready
		if !quiet {
			fmt.Println("input", name, "is now connected")
		}
	}
	for name, _ := range outEndpoints {
		ep := outEndpoints[name] //TODO not sure if this is necessary
		if debug {
			fmt.Println("waiting for ready from output", name)
		}
		<-ep.Ready
		if !quiet {
			fmt.Println("output", name, "is now connected")
		}
	}

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
				// setup direct copy without processing (since unframed)
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
			go handleInputEndpoint(inEndpoints[inEndpoint], debug, quiet, cin)
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
			bufr := bufio.NewReader(cout)
			for {
				if frame, err := flowd.ParseFrame(bufr); err != nil {
					if err == io.EOF {
						fmt.Println("EOF from component stdout. Closing.")
					} else {
						fmt.Println("ERROR parsing frame from component stdout:", err, "- closing.")
					}
				} else { // frame complete now
					if debug {
						fmt.Println("STDOUT received frame type", frame.Type, "data type", frame.BodyType, "for port", frame.Port, "with body:", (string)(*frame.Body))
					}

					// write out to network
					//TODO error feedback for unknown/unconnected output ports
					if e, exists := outEndpoints[frame.Port]; exists {
						//TODO rewrite frame.Port to match the other side's input port name - and write the marshaled frame, not the buf

						// marshal
						//TODO optimize; could directly marshal to e.Conn, but would lose info how many bytes were written
						var outbuf bytes.Buffer
						outw := bufio.NewWriter(&outbuf)
						if err := frame.Marshal(outw); err != nil {
							fmt.Println("net out: ERROR: marshalling frame:", err.Error(), "- closing.")
							outEndpoints[frame.Port].Close()
							//TODO return as well = close down all output operations or allow one output to fail?
						}

						// write
						if nout, err := e.Conn.Write(outbuf.Bytes()); err != nil {
							fmt.Println("net out: ERROR: sending to output endpoint", frame.Port, ":", err.Error(), "- closing.")
							outEndpoints[frame.Port].Close()
							//TODO return as well = close down all output operations or allow one output to fail?
						} else if nout < outbuf.Len() {
							fmt.Println("net out: ERROR: short send to output endpoint", frame.Port, ": only", nout, "of", outbuf.Len(), "bytes written - closing.")
							outEndpoints[frame.Port].Close()
						} else {
							//TODO having two outputs say the same seems useless
							if debug {
								fmt.Println("net out wrote", outbuf.Len(), "bytes to port", frame.Port, "over network")
							}
							if !quiet {
								fmt.Println("out xfer", outbuf.Len(), "bytes to", frame.Port, "=", outEndpoints[frame.Port].Conn.RemoteAddr())
							}
						}
					} else {
						fmt.Println("net out: ERROR: component tried sending to undeclared port. Exiting.")
						outEndpoints.Close()
						return
					}
				}
			}
		}()
	}

	// trigger on signal (SIGHUP, SIGUSR1, SIGUSR2, etc.) to reconnect, reconfigure etc.
	//TODO

	// declare network ports
	//TODO

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

func handleInputEndpoint(ep *inputEndpoint, debug bool, quiet bool, cin io.WriteCloser) {
	buf := make([]byte, bufSize)
	var nPrev int
	for {
		// read from connection
		n, err := ep.Conn.Read(buf[nPrev:])
		// NOTE: above can lead to reading only parts of an UDP packet even if buffer is large enough
		// ###
		if err != nil {
			if err == io.EOF {
				_, portStr, _ := net.SplitHostPort(ep.Url.Host)
				if portStr != "0" {
					// can listen again since port is same (would have to change zeroconf announce)
					fmt.Println("EOF from network input", ep.Url.Fragment, "- listening again.")

					//start listening again to allow re-connection
					ep.ListenAgain(func() {
						handleInputEndpoint(ep, debug, quiet, cin)
					})
				} else {
					fmt.Println("EOF from network input", ep.Url.Fragment, "- closing.")
				}
			} else {
				fmt.Println("ERROR: receiving from network input:", err, "Closing.")
			}
			ep.Conn.Close()
			return
		}
		if debug {
			fmt.Println("net in received", n, "bytes from", ep.Conn.RemoteAddr()) //" with contents:", string(buf[0:n]))
		}
		// decode frame
		//bufReader := bytes.NewReader(buf[0 : n+nPrev])
		bufReader := bufio.NewReader(bytes.NewReader(buf[0 : n+nPrev])) //TODO optimize; could directly ParseFrame from conn if it was TCP
		//flowd.ParseFrame(ep.Conn)
		// ### remove re-assembly, just join it to buffer (TODO an information packet can be really large -> something streaming)
		// TODO could do packet re-assembly only if connection is UDPConn or UnixDatagramConn using type switch i := conn.(type) {}
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

			// check frame Port header field if it matches the name of this input endpoint
			//FIXME which side does the header field rewriting? - better the sending side.
			if ep.Url.Fragment != fr.Port {
				fmt.Println("net in: WARNING: frame for wrong/undeclared port", fr.Port, "- expected:", ep.Url.Fragment, " - discarding.")
				// discard frame
			} // else {	//TODO actually enforce this and make error in msg above
			// forward frame to component
			cin.Write(buf[0 : n+nPrev]) // NOTE: simply sending in whole buf would make body JSON decoder error because of \x00 bytes beyond payload
			//TODO two outputs saying just about the same seems useless
			if debug {
				fmt.Println("STDIN wrote", nPrev+n, "bytes to component stdin")
			}
			if !quiet {
				fmt.Println("in xfer", nPrev+n, "bytes from", ep.Conn.RemoteAddr())
			}

			// reset previous packet size
			nPrev = 0
		}
	}
}
