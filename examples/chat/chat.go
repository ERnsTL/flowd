package main

import (
	"bufio"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	/*
		// read list of output ports from program arguments
		outPorts := os.Args[1:]
		if len(outPorts) == 0 {
			// get configuration from IIP = initial information packet/frame
			fmt.Fprintln(os.Stderr, "wait for IIP")
			if iip, err := flowd.GetIIP("CONF"); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
				os.Exit(1)
			} else {
				outPorts = strings.Split(iip, ",")
				if len(outPorts) == 0 {
					fmt.Fprintln(os.Stderr, "ERROR: no output ports names given in IIP, format is [port],[port],[port]...")
					os.Exit(1)
				}
			}
		}
		fmt.Fprintln(os.Stderr, "got output ports", outPorts)
	*/

	var frame *flowd.Frame //TODO why is this pointer of Frame?
	var err error
	bufr := bufio.NewReader(os.Stdin)

	var state string
	var username string

	for {
		// read frame
		frame, err = flowd.ParseFrame(bufr)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}
		inLine := strings.TrimSpace(string(frame.Body))

		switch state {
		case "", "login_user":
			if len(inLine) > 0 {
				// save username, ask for password
				username = inLine
				state = "login_pass"
				Respond(frame, "password: ")
			} else {
				frame.Body = []byte("username: ")
			}

		case "login_pass":
			if len(inLine) > 0 {
				if username == "ernst" && inLine == "password" {
					state = "cmdline"
					Respond(frame, fmt.Sprintf("ok, you are in.\n\n> "))
				} else {
					time.Sleep(2 * time.Second)
					state = "login_user"
					Respond(frame, fmt.Sprintf("wrong.\n\nusername: "))
				}
			} else {
				Respond(frame, fmt.Sprintf("You did not enter anything!\npassword: "))
			}

		case "cmdline":
			switch inLine {
			case "exit", "quit", "logout":
				state = "login_user"
				Respond(frame, fmt.Sprintf("kthxbye!\n"))
				//TODO send close request as response
			case "":
				Respond(frame, "> ")
			default:
				Respond(frame, fmt.Sprintf("unimplemented: %s\n> ", inLine))
			}
		}

		// send it to output port
		frame.Port = "OUT"
		if err := frame.Marshal(os.Stdout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
		}
	}
}

func Respond(f *flowd.Frame, text string) {
	f.Body = []byte(text)
}
