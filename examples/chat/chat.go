package main

import (
	"bufio"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
)

type ChatClient struct {
	state    string
	username string
}

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
	var id string // connection ID
	bufr := bufio.NewReader(os.Stdin)

	clients := map[string]*ChatClient{} // key = connection ID

	// prepare commonly-used variables
	closeCommand := &flowd.Frame{
		Port:     "OUT",
		Type:     "data",
		BodyType: "TCPCloseConnection",
		Extensions: map[string]string{
			"Tcp-Id": "",
		},
	}

	for {
		// read frame
		frame, err = flowd.ParseFrame(bufr)
		// get connection ID
		id = frame.Extensions["Tcp-Id"]

		// check connection ID - known?
		// NOTE: not necessary, because we should receive a n
		/*
			if id, found = frame.Extensions["Tcp-Id"]; !found {

			}
		*/
		// handle according to BodyType
		if frame.BodyType == "OpenNotification" {
			fmt.Fprintf(os.Stderr, "%s: got open notification, making new entry.\n", id)
			// make new entry
			clients[id] = &ChatClient{}
			// continue and send welcome message
		} else if frame.BodyType == "CloseNotification" {
			fmt.Fprintf(os.Stderr, "%s: got close notification, removing.\n", id)
			// remove entry
			delete(clients, id)
			// pass
			continue
		}

		// extract entered line
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}
		inLine := strings.TrimSpace(string(frame.Body))

		switch clients[id].state {
		case "", "login_user":
			if len(inLine) > 0 {
				// save username, ask for password
				clients[id].username = inLine
				clients[id].state = "login_pass"
				Respond(frame, "password: ")
			} else {
				frame.Body = []byte("username: ")
			}

		case "login_pass":
			if len(inLine) > 0 {
				if clients[id].username == "ernst" && inLine == "password" {
					clients[id].state = "cmdline"
					Respond(frame, fmt.Sprintf("ok, you are in.\n\n> "))
					// write to log
					fmt.Fprintf(os.Stderr, "%s: user %s logged in.\n", id, "ernst")
				} else {
					time.Sleep(2 * time.Second)
					clients[id].state = "login_user"
					Respond(frame, fmt.Sprintf("wrong.\n\nusername: "))
				}
			} else {
				Respond(frame, fmt.Sprintf("You did not enter anything!\npassword: "))
			}

		case "cmdline":
			switch inLine {
			case "exit", "quit", "logout":
				// send close request
				fmt.Fprintf(os.Stderr, "%s: sending close command.\n", id)
				frame = closeCommand
				frame.Extensions["Tcp-Id"] = id
				Respond(frame, fmt.Sprintf("kthxbye!\n"))
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
