package main

import (
	"bufio"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
)

const prompt = "> "

// ChatClient holds the state of a connected client
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
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()

	clients := map[string]*ChatClient{} // key = connection ID

	// prepare commonly-used variables
	closeCommand := &flowd.Frame{
		Port:     "OUT",
		Type:     "data",
		BodyType: "CloseConnection",
		Extensions: map[string]string{
			"conn-id": "",
		},
	}

	for {
		// read frame
		frame, err = flowd.ParseFrame(netin)
		// get connection ID
		id = frame.Extensions["conn-id"]

		// check connection ID - known?
		// NOTE: not necessary, because we should receive a n
		/*
			if id, found = frame.Extensions["conn-id"]; !found {

			}
		*/
		// handle according to BodyType
		if frame.BodyType == "OpenNotification" {
			fmt.Fprintf(os.Stderr, "%s: got open notification, making new entry.\n", id)
			// make new entry
			clients[id] = &ChatClient{}
			// continue and send welcome message
			frame.BodyType = "ChatMessage"
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
				frame.Extensions["conn-id"] = id
				Respond(frame, fmt.Sprintln("kthxbye!"))
			case "help":
				Respond(frame, fmt.Sprintf("commands: exit, quit, logout\n%s", prompt))
			case "":
				Respond(frame, prompt)
			default:
				Respond(frame, fmt.Sprintf("unimplemented: %s\n%s", inLine, prompt))
			}
		}

		// send it to output port
		frame.Port = "OUT"
		if err = frame.Marshal(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
		}
		// send it now (flush)
		if err = netout.Flush(); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
		}
	}
}

// Respond is a helper function to quickly set a frame body from a string
func Respond(f *flowd.Frame, text string) {
	f.Body = []byte(text)
}
