package main

import (
	"bufio"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"strings"

	"github.com/ERnsTL/UnixFBP/libunixfbp"
	"github.com/ERnsTL/flowd/libflowd"
)

const bufSize = 65536

var netout *bufio.Writer //TODO is this concurrently useable? better give as param to handler function?

func main() {
	// get configuration from arguments = Unix IIP
	var operatingMode OperatingMode
	var framing, retry bool
	var cmdargs []string
	unixfbp.DefFlags()
	flag.Var(&operatingMode, "mode", "operating mode: one (command instance handling all IPs) or each (IP handled by new instance)")
	flag.BoolVar(&framing, "framing", true, "true = frame mode, false = send frame body to command STDIN, frame the data from command STDOUT")
	flag.BoolVar(&retry, "retry", false, "retry/restart command on non-zero return code")
	flag.Parse()
	if flag.NArg() == 0 {
		fmt.Fprintln(os.Stderr, "ERROR: missing command to run")
		printUsage()
		flag.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	cmdargs = flag.Args()

	// connect to FBP network
	netin, _, err := unixfbp.OpenInPort("IN")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	netout, _, err = unixfbp.OpenOutPort("OUT")
	if err != nil {
		fmt.Println("ERROR:", err)
		os.Exit(2)
	}
	defer netout.Flush()

	fmt.Fprintln(os.Stderr, "starting up, command is", strings.Join(cmdargs, " "))

	// prepare subprocess variables
	var cmd *exec.Cmd
	var cin io.WriteCloser
	var cout io.ReadCloser

	//TODO implement timeout on subprocess
	/*
		// start
		cmd := exec.Command("sleep", "5")
		if err := cmd.Start(); err != nil {
			panic(err)
		}

		// wait or timeout
		donec := make(chan error, 1)
		go func() {
			donec <- cmd.Wait()
		}()
		select {
		case <-time.After(3 * time.Second):
			cmd.Process.Kill()
			fmt.Println("timeout")
		case <-donec:
			fmt.Println("done")
		}
	*/
	//TODO implement retry/restart in one mode
	//TODO implement retry/restart in each mode

	// main work loops
	switch operatingMode {
	case One:
		// start command as subprocess, with arguments
		cmd, cin, cout = startCommand(cmdargs)
		defer cout.Close()
		defer cin.Close()

		// handle subprocess output
		go handleCommandOutput(cout)

		// handle subprocess input
		if framing == true {
			// setup direct copy without processing (since already framed)
			if _, err := io.Copy(cin, netin); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: receiving from FBP network:", err, "Closing.")
				os.Stdin.Close()
				return
			}
		} else {
			// loop: read frame, write body to subprocess
			copyFrameBodies(cin, netin)
		}
	case Each:
		// prepare variables
		var frame *flowd.Frame
		var bufcin *bufio.Writer
		var err error

		for {
			// read frame
			frame, err = flowd.Deserialize(netin)
			if err != nil {
				fmt.Fprintln(os.Stderr, err)
			} else if unixfbp.Debug {
				fmt.Fprintln(os.Stderr, "received frame:", string(frame.Body))
			}

			// handle each frame with a new instance
			go func(frame *flowd.Frame) {
				// start new command instance
				cmd, cin, cout = startCommand(cmdargs)

				// handle subprocess output
				if framing {
					go handleCommandOutput(cout)
				} else {
					go handleCommandOutputRaw(cout)
				}

				// forward frame or frame body to subprocess
				if framing {
					// prepare buffered writer (required by Marshal TODO optimize)
					if bufcin == nil {
						// first use
						bufcin = bufio.NewWriter(cin)
					} else {
						// re-use existing one (saves the buffer allocation)
						bufcin.Reset(cin)
					}
					// write frame
					if err := frame.Serialize(bufcin); err != nil {
						fmt.Fprintln(os.Stderr, "ERROR: marshaling frame to command STDIN:", err, "- Exiting.")
						os.Exit(3)
					}
					// flush frame
					if err := bufcin.Flush(); err != nil {
						fmt.Fprintln(os.Stderr, "ERROR: flusing frame to command STDIN:", err, "- Exiting.")
						os.Exit(3)
					}
				} else {
					// write frame body
					if _, err := cin.Write(frame.Body); err != nil {
						fmt.Fprintln(os.Stderr, "ERROR: writing frame body to command STDIN:", err, "- Exiting.")
						os.Exit(3)
					} else if unixfbp.Debug {
						fmt.Fprintln(os.Stderr, "sent frame body to subcommand STDIN:", string(frame.Body))
					}
					// done sending = close command STDIN
					if err := cin.Close(); err != nil {
						fmt.Fprintln(os.Stderr, "ERROR: could not close command STDIN after writing frame:", err, "- Exiting.")
						os.Exit(3)
					}
				}

				// wait for subprocess to finish
				if err := cmd.Wait(); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: command exited with error:", err, "- Exiting.")
					os.Exit(3)
				}
			}(frame)
		}
	default:
		fmt.Fprintln(os.Stderr, "ERROR: main loop: unknown operating mode - Exiting.")
		os.Exit(3)
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "IIP format: [flags] [cmdpath] [args]...")
}

func startCommand(cmdargs []string) (cmd *exec.Cmd, cin io.WriteCloser, cout io.ReadCloser) {
	var err error
	cmd = exec.Command(cmdargs[0], cmdargs[1:]...)
	cout, err = cmd.StdoutPipe()
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: could not allocate pipe from command stdout:", err)
	}
	cin, err = cmd.StdinPipe()
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: could not allocate pipe to command stdin:", err)
	}
	cmd.Stderr = os.Stderr
	err = cmd.Start()
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: starting command:", err)
		os.Exit(3)
	}
	return
}

func handleCommandOutput(cout io.ReadCloser) {
	bufr := bufio.NewReader(cout)
	for {
		frame, err := flowd.Deserialize(bufr)
		if err != nil {
			if err == io.EOF {
				fmt.Fprintln(os.Stderr, "EOF from command stdout. Exiting.")
			} else {
				fmt.Fprintln(os.Stderr, "ERROR parsing frame from command stdout:", err, "- Exiting.")
			}
			cout.Close()
			return
		}

		// got a complete frame
		if unixfbp.Debug {
			fmt.Fprintln(os.Stderr, "STDOUT received frame type", frame.Type, "and data type", frame.BodyType, "for port", frame.Port, "with body:", (string)(frame.Body)) //TODO what is difference between this and string(frame.Body) ?
		}
		// set correct port
		frame.Port = "OUT"
		// send into FBP network
		if err := frame.Serialize(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: could not send frame to FBP network:", err, "- Closing.")
			os.Stdout.Close()
			return
		}
	}
}

func handleCommandOutputRaw(cout io.ReadCloser) {
	// prepare readers and variables
	bufr := bufio.NewReader(cout)
	buf := make([]byte, bufSize)
	frame := &flowd.Frame{
		//Port:     "OUT",
		Type:     "data",
		BodyType: "Data",
	}
	// read loop
	for {
		nbytes, err := bufr.Read(buf)
		if err != nil {
			if err == io.EOF {
				fmt.Fprintln(os.Stderr, "WARNING: EOF from command:", err, "- Closing.")
				cout.Close()
				return
			}
			// other error
			fmt.Fprintln(os.Stderr, "ERROR: reading from command STDOUT:", err, "- Closing.")
			cout.Close()
			return
		}

		// frame command output and send into FBP network
		frame.Body = buf[0:nbytes]
		if err := frame.Serialize(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: could not send frame to FBP network:", err, "- Closing.")
			os.Stdout.Close()
			return
		}

		// flush
		if bufr.Buffered() == 0 {
			if err := netout.Flush(); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
			}
		}
	}
}

func copyFrameBodies(cin io.WriteCloser, bufr *bufio.Reader) {
	var frame *flowd.Frame
	var err error
	for {
		// read frame
		frame, err = flowd.Deserialize(bufr)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		fmt.Fprintln(os.Stderr, "got packet:", string(frame.Body))

		// forward frame body to prepared command instance
		if _, err = cin.Write(frame.Body); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: writing frame body to command STDIN:", err, "- Exiting.")
			os.Exit(3)
		}
	}
}
