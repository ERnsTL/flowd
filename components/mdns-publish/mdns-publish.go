package main

import (
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"strconv"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/ERnsTL/flowd/libunixfbp"
)

//TODO TXT records (tags) can be multiple
//TODO use a mDNS Go library - already have that in the drawer

func main() {
	// get configuration from arguments = Unix IIP
	var name, maintype, protocol, tagKV, subtype string
	var port int
	unixfbp.DefFlags()
	flag.StringVar(&name, "name", "My flowd Webserver", "service description")
	flag.StringVar(&maintype, "type", "_flowd", "service type in mDNS/Zeroconf/Bonjour format")
	flag.StringVar(&protocol, "proto", "tcp", "protocol to use (tcp or udp)")
	flag.IntVar(&port, "port", 8080, "service port number")
	flag.StringVar(&tagKV, "tag", "", "key-value tag resp. TXT record in the form \"key=value\" (optional)")
	flag.StringVar(&subtype, "subtype", "", "additional service subtype in mDNS/Zeroconf/Bonjour format, eg. _web._sub._flowd-olc (optional)")
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "ERROR: unexpected free argument(s) - exiting.")
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

	// start publisher
	var pub *exec.Cmd
	go func() {
		if unixfbp.Debug {
			fmt.Fprintln(os.Stderr, "starting publisher")
		}
		const arg0 = "avahi-publish-service"
		args := []string{"--service", name, maintype + "._" + protocol, strconv.Itoa(port)}
		if tagKV != "" {
			args = append(args, tagKV)
		}
		if subtype != "" {
			args = append(args, "--subtype", subtype+"._"+protocol)
		}
		if unixfbp.Debug {
			// print command-line
			fmt.Fprintln(os.Stderr, "commandline:", arg0, args)
		}
		pub = exec.Command(arg0, args...)
		out, err := pub.CombinedOutput()
		fmt.Fprintln(os.Stderr, string(out))
		if err != nil {
			fmt.Println("ERROR:", err)
			os.Exit(4)
		} else {
			os.Exit(0)
		}
	}()

	// main loop
	fmt.Fprintln(os.Stderr, "running")
	for {
		// wait for EOF as exit signal, discarding frames until then
		_, err = flowd.Deserialize(netin)
		if err != nil {
			if err == io.EOF {
				// signal for closing
				fmt.Fprintln(os.Stderr, "EOF")
				//pub.Process.Kill() // otherwise it keeps running in the background (not true)
				return
			}
		}
		if unixfbp.Debug {
			fmt.Fprintln(os.Stderr, "got a packet")
		}
	}
}

func checkError(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR:", err)
		os.Exit(2)
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Arguments: [-debug] [-quiet] -name <service-name> -maintype <service-type> -port <port-number> [-tag <key=value>] [-subtype <sub-type>]")
}
