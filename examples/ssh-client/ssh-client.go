package main

import (
	"bufio"
	"bytes"
	"crypto/hmac"
	"crypto/sha1"
	"encoding/base64"
	"flag"
	"fmt"
	"io"
	"io/ioutil"
	"net"
	"net/url"
	"os"
	"strings"

	"github.com/kballard/go-shellquote"
	"golang.org/x/crypto/ssh"
	sshagent "golang.org/x/crypto/ssh/agent"

	flowd "github.com/ERnsTL/flowd/libflowd"
)

const bufSize = 65536

var (
	debug  bool
	quiet  bool
	netout *bufio.Writer //TODO safe to use concurrently?
)

func main() {
	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout = bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	iip, err := flowd.GetIIP("CONF", netin)
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- exiting.")
		os.Exit(1)
	}
	// parse IIP
	var retry, bridge, useAgent, subsystem bool
	var pkPath, khPath, hostkeyLine string
	var operatingMode OperatingMode
	iipSplit, err := shellquote.Split(iip)
	checkError(err)
	//TODO add flag for subsystem -> session.RequestSubsystem instead of session.Run()
	flags := flag.NewFlagSet("ssh-client", flag.ContinueOnError)
	flags.Var(&operatingMode, "mode", "operating mode: one (command instance handling all IPs) or each (IP handled by new instance)")
	flags.BoolVar(&useAgent, "agent", true, "use ssh-agent as key source")
	flags.StringVar(&pkPath, "i", "", "use private key file, e.g. ~/.ssh/id_ed25519|ecdsa|rsa|dsa")
	flags.StringVar(&khPath, "knownhosts", os.ExpandEnv("$HOME/.ssh/known_hosts"), "path to SSH known_hosts file containing known host keys, may select wrong algo if multiple")
	flags.StringVar(&hostkeyLine, "hostkey", "", "use hostkey line for server verification, put in quotes, get it using ssh-keyscan or from /etc/ssh/ssh_host_*_key.pub, must be for algo actually used")
	flags.BoolVar(&subsystem, "subsystem", false, "request invocation of a subsystem on the remote system; which, is specified as the remote command")
	flags.BoolVar(&bridge, "bridge", false, "bridge mode, true = forward frames from/to FBP network, false = send frame body over SSH, frame data from SSH")
	flags.BoolVar(&retry, "retry", false, "retry connection and try to reconnect")
	flags.BoolVar(&debug, "debug", false, "give detailed event output")
	flags.BoolVar(&quiet, "quiet", false, "no informational output except errors")
	//if err = flags.Parse(strings.Split(iip, " ")); err != nil {
	if err = flags.Parse(iipSplit); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR parsing IIP arguments - exiting.")
		printUsage()
		//flags.PrintDefaults() // prints to STDERR
		os.Exit(2)
	}
	if flags.NArg() < 2 {
		fmt.Fprintln(os.Stderr, "ERROR: missing remote command")
		printUsage()
		flags.PrintDefaults()
		os.Exit(2)
	}
	cmdargs := flags.Args()[1:]
	if debug {
		fmt.Fprintf(os.Stderr, "cmdargs=%v\n", cmdargs)
	}

	//TODO implement
	if retry {
		fmt.Fprintln(os.Stderr, "ERROR: flag -retry currently unimplemented - exiting.")
		os.Exit(2)
	}
	//TODO implement
	if subsystem {
		fmt.Fprintln(os.Stderr, "ERROR: flag -s currently unimplemented - exiting.")
		os.Exit(2)
	}

	// parse remote address as URL
	//TODO parse something like ssh://user@sampleserver.com
	// NOTE: add double slashes after semicolon
	remoteURLStr := flags.Args()[0]
	if !strings.HasPrefix(remoteURLStr, "tcp") {
		remoteURLStr = "tcp://" + remoteURLStr
	}
	remoteURL, err := url.ParseRequestURI(remoteURLStr)
	checkError(err)
	remoteNetwork := remoteURL.Scheme
	var username, password string
	if remoteURL.User != nil {
		password, _ = remoteURL.User.Password()
		username = remoteURL.User.Username()
	}
	if debug {
		fmt.Fprintf(os.Stderr, "Scheme=%s, Opaque=%s, Host=%s, Path=%s, User=%s, Pass=%s\n", remoteURL.Scheme, remoteURL.Opaque, remoteURL.Host, remoteURL.Path, username, password)
	}
	if remoteURL.Path != "" {
		fmt.Fprintln(os.Stderr, "ERROR: remote URL must not contain path")
		printUsage()
		flags.PrintDefaults()
		os.Exit(2)
	}
	remoteHost := remoteURL.Host
	// add default port if none given
	if !strings.ContainsRune(remoteHost, ':') {
		remoteHost = remoteHost + ":22"
	}

	// list of available auth methods
	// NOTE: keys and key sources are of type ssh.Signer, which need to be turned
	// into ssh.AuthMethod using ssh.Password(), ssh.PublicKeysCallback(), ssh.PublicKeys() ...
	//TODO any difference between putting public keys from agent and file as separate
	//	AuthMethods or compiling list of keys/signers, then putting them into one AuthMethod?
	var authMethods []ssh.AuthMethod
	if password != "" {
		authMethods = append(authMethods, ssh.Password(password))
	}
	if useAgent {
		agentConn, err := net.Dial("unix", os.Getenv("SSH_AUTH_SOCK"))
		checkError(err)
		authMethods = append(authMethods, ssh.PublicKeysCallback(sshagent.NewClient(agentConn).Signers))
	}
	if pkPath != "" {
		// read identity file / private key
		pkFile, err := os.Open(pkPath)
		checkError(err)
		defer pkFile.Close()
		pkBytes, err := ioutil.ReadAll(pkFile)
		checkError(err)
		pkSigner, err := ssh.ParsePrivateKey(pkBytes)
		checkError(err)
		pkAuth := ssh.PublicKeys(pkSigner)
		authMethods = append(authMethods, pkAuth)
	}

	sshConfig := &ssh.ClientConfig{
		User: username,
		Auth: authMethods,
		// NOTE: hostkey verification see https://bridge.grumpy-troll.org/2017/04/golang-ssh-security/
		//HostKeyCallback: ssh.InsecureIgnoreHostKey(),
	}

	// get host key
	if hostkeyLine != "" {
		// read from arguments
		if debug {
			fmt.Fprintf(os.Stderr, "parsing hostkey line '%s'\n", hostkeyLine)
		}
		//pk, err := ssh.ParsePublicKey([]byte(hostkeyLine))
		//TODO ^ does not work :-| short read - what format is expected there?
		pk, _, _, _, err := ssh.ParseAuthorizedKey([]byte(hostkeyLine))
		checkError(err)
		sshConfig.HostKeyCallback = ssh.FixedHostKey(pk)
	} else if khPath != "" {
		// read known_hosts file
		if debug {
			fmt.Fprintf(os.Stderr, "using '%s' for hostkey verification\n", khPath)
		}
		khFile, err := os.Open(khPath)
		checkError(err)
		defer khFile.Close()
		khBytes, err := ioutil.ReadAll(khFile)
		checkError(err)
		if len(khBytes) == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: filepath given in flag -knownhosts is empty")
			os.Exit(1)
		}
		// set hostkey callback - will be called once hostname and server public key is known
		sshConfig.HostKeyCallback = func(hostname string, remote net.Addr, key ssh.PublicKey) error {
			//TODO this function gets called once on connect, and for each session = program run
			// -> cache check results in a map with key.Marshal() as key or similar
			// NOTE: need hostname without port and without [] braces, contrary to suggestions in many places
			hostname = strings.SplitN(hostname, ":", 2)[0]
			if debug {
				fmt.Fprintf(os.Stderr, "host key callback shall check host '%s'\n", hostname)
			}
			//hasher := sha1.New() // NOTE: hasher.Sum() returns []byte, but sha1.Sum() returns [size]byte
			var foundHostKey ssh.PublicKey
			rest := khBytes
		searchKey:
			for len(rest) > 0 {
				// get next public key from known_hosts file
				marker, hosts, pk, _, restTmp, err := ssh.ParseKnownHosts(rest)
				rest = restTmp  //TODO optimize
				checkError(err) // TODO == IE.EOF -> none found
				if marker == "revoked" {
					continue
				} else if marker == "cert-authority" {
					continue
				}
				if pk.Type() != key.Type() {
					continue
				}
				for _, hostKnown := range hosts {
					// check for hashed hostname
					// NOTE: structure @ https://security.stackexchange.com/questions/56268/ssh-benefits-of-using-hashed-known-hosts
					// NOTE: also structure @ https://serverfault.com/questions/331080/what-do-the-different-parts-of-known-hosts-entries-mean
					// NOTE: multiple entries @ https://askubuntu.com/questions/446878/why-do-ive-two-entries-per-server-in-known-hosts-file
					if strings.HasPrefix(hostKnown, "|1|") {
						// split and extract salt and hash
						hostHashedParts := strings.SplitN(hostKnown, "|", 4)
						if hostHashedParts == nil || len(hostHashedParts) != 4 {
							return fmt.Errorf("malformed known_hosts file: hashed hostname format wrong")
						}
						hashSaltKnownBase64 := hostHashedParts[2]
						hostHashedKnownBase64 := hostHashedParts[3]
						// base64 decode
						hashSaltKnown, err := base64.StdEncoding.DecodeString(hashSaltKnownBase64)
						checkError(err)
						hostHashedKnown, err := base64.StdEncoding.DecodeString(hostHashedKnownBase64)
						checkError(err)
						// generate local of hostname hash
						mac := hmac.New(sha1.New, hashSaltKnown)
						mac.Write([]byte(hostname))
						expectedMAC := mac.Sum(nil)
						// check sum
						if hmac.Equal(hostHashedKnown, expectedMAC) {
							foundHostKey = pk
							break searchKey
						}
					} else {
						// unhashed hostnames - check hostname using string equality
						if hostKnown == hostname {
							foundHostKey = pk
							break searchKey
						}
					}
				}
			}
			if foundHostKey == nil {
				return fmt.Errorf("no host key found in known_hosts file")
			}

			// check actual host key
			if debug {
				fmt.Fprintln(os.Stderr, "host key found, checking...")
			}
			if !bytes.Equal(key.Marshal(), foundHostKey.Marshal()) {
				return fmt.Errorf("known host key found for active key type, but mismatch")
			}

			if !quiet {
				fmt.Fprintln(os.Stderr, "host key verified")
			}
			return nil
		}
	} else {
		// have no host key
		fmt.Fprintln(os.Stderr, "ERROR: no host key source given, see usage")
		os.Exit(1)
	}

	// connect to SSH server
	if !quiet {
		fmt.Fprintln(os.Stderr, "connecting to server")
	}
	connServer, err := ssh.Dial(remoteNetwork, remoteHost, sshConfig)
	checkError(err)
	defer connServer.Close()
	//TODO unreachable defer agentConn.Close()
	if !quiet {
		fmt.Fprintln(os.Stderr, "connected")
	}

	//TODO implement timeout on subprocess
	//TODO implement retry/restart in one mode
	//TODO implement retry/restart in each mode

	// main work loops
	var session *ssh.Session
	var cin io.WriteCloser
	var cout io.Reader
	switch operatingMode {
	case One:
		// start command as subprocess, with arguments
		session, cin, cout = startCommand(connServer, cmdargs)
		//defer cin.Close()

		// handle subprocess output
		if bridge {
			go handleCommandOutput(debug, cout)
		} else {
			go handleCommandOutputRaw(debug, cout)
		}

		// handle subprocess input
		if bridge {
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
		var frame *flowd.Frame //TODO why is this pointer to Frame?
		var bufcin *bufio.Writer
		var err error

		for {
			// read frame
			frame, err = flowd.ParseFrame(netin)
			if err != nil {
				fmt.Fprintln(os.Stderr, err)
			} else if debug {
				fmt.Fprintln(os.Stderr, "received frame:", string(frame.Body))
			}

			// handle each frame with a new instance
			go func(frame *flowd.Frame) {
				// start new command instance
				session, cin, cout = startCommand(connServer, cmdargs)

				// handle subprocess output
				if bridge {
					go handleCommandOutput(debug, cout)
				} else {
					go handleCommandOutputRaw(debug, cout)
				}

				// forward frame or frame body to subprocess
				if bridge {
					// prepare buffered writer (required by Marshal TODO optimize)
					if bufcin == nil {
						// first use
						bufcin = bufio.NewWriter(cin)
					} else {
						// re-use existing one (saves the buffer allocation)
						bufcin.Reset(cin)
					}
					// write frame
					if err := frame.Marshal(bufcin); err != nil {
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
					} else if debug {
						fmt.Fprintln(os.Stderr, "sent frame body to subcommand STDIN:", string(frame.Body))
					}
					// done sending = close command STDIN
					if err := cin.Close(); err != nil {
						fmt.Fprintln(os.Stderr, "ERROR: could not close command STDIN after writing frame:", err, "- Exiting.")
						os.Exit(3)
					}
				}

				// wait for subprocess to finish
				exitStatus := session.Wait()
				if exitCode, err := getExitCode(exitStatus); err != nil {
					fmt.Fprintln(os.Stderr, "ERROR: command exited with error:", err, "- Exiting.")
					os.Exit(3)
				} else {
					if !quiet {
						fmt.Fprintln(os.Stderr, "command exited with code", exitCode)
					}
				}
			}(frame)
		}
	default:
		fmt.Fprintln(os.Stderr, "ERROR: main loop: unknown operating mode - Exiting.")
		os.Exit(3)
	}
}

func checkError(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR:", err)
		os.Exit(2)
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "IIP format: [flags] [ssh://][user=me]:[password]@[host][:port=22] [cmdpath] [args...]")
}

func startCommand(connServer *ssh.Client, cmdargs []string) (session *ssh.Session, cin io.WriteCloser, cout io.Reader) {
	// make session for remote command
	// NOTE: each session accepts only one call to Run, Start, Shell, Output or CombinedOutput
	if !quiet {
		fmt.Fprintln(os.Stderr, "asking for new session")
	}
	var err error
	session, err = connServer.NewSession()
	checkError(err)
	//defer session.Close()	- must not do this here in this function
	if !quiet {
		fmt.Fprintln(os.Stderr, "session established")
	}

	// request pseudo-terminal (pty)
	/*
		terminalConfig := ssh.TerminalModes{
			53:  0,     // echo off
			128: 14400, // input speed = 14.4 kbaud
			129: 14400, // output speed = 14.4 kbaud
		}
		err = session.RequestPty("xterm", 80, 40, terminalConfig)
		checkError(err)
	*/

	// prepare stdin, stdout and stderr targets
	//cout, coutw, err := os.Pipe()
	cout, err = session.StdoutPipe()
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: could not allocate pipe from session stdout:", err)
	}
	//session.Stdout = coutw
	cin, err = session.StdinPipe()
	//cinr, cin, err := os.Pipe()
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR: could not allocate pipe to session stdin:", err)
	}
	//session.Stdin = cinr
	session.Stderr = os.Stderr

	// start the remote command
	// NOTE: Run = Start + Wait
	// TODO optimize - first split in IIP and flags, then joined here
	if !quiet {
		fmt.Fprintf(os.Stderr, "starting remote command %v\n", cmdargs)
	}
	cmdargsStr := strings.Join(cmdargs, " ")
	err = session.Start(cmdargsStr)
	checkError(err)

	return
}

//TODO optimize code or make it unnecessary
func getExitCode(err error) (int, error) {
	// check exit status
	var exitCode int
	/* TODO maybe only exit status is interesting
	if exitError, ok := r.Error.(*ssh.ExitError); ok {
		r.ExitStatus = exitError.ExitStatus()
	}
	*/
	if err != nil {
		switch err := err.(type) {
		case *ssh.ExitError:
			exitCode = err.ExitStatus()
		case *ssh.ExitMissingError:
			if debug {
				fmt.Fprintln(os.Stderr, "program exited, error status missing")
			}
			return -1, fmt.Errorf("program exited, error status missing")
		default:
			// TODO check detailed for IO errors, check ssh package source code
			fmt.Fprintf(os.Stderr, "other error occured: %v\n", err)
			return -1, err
		}
	}
	if exitCode != 0 {
		if !quiet {
			fmt.Fprintf(os.Stderr, "program exited with status code %d\n", exitCode)
		}
	} else {
		if !quiet {
			fmt.Fprintln(os.Stderr, "program exited normally")
		}
	}
	return exitCode, nil
}

//TODO pretty much 1:1 copy from cmd handleCommandOutput() -> reuse?
func handleCommandOutput(debug bool, cout io.Reader) {
	bufr := bufio.NewReader(cout)
	for {
		frame, err := flowd.ParseFrame(bufr)
		if err != nil {
			if err == io.EOF {
				fmt.Fprintln(os.Stderr, "EOF from command stdout. Exiting.")
			} else {
				fmt.Fprintln(os.Stderr, "ERROR parsing frame from command stdout:", err, "- Exiting.")
			}
			//cout.Close()
			return
		}

		// got a complete frame
		if debug == true {
			fmt.Fprintln(os.Stderr, "STDOUT received frame type", frame.Type, "and data type", frame.BodyType, "for port", frame.Port, "with body:", (string)(frame.Body)) //TODO what is difference between this and string(frame.Body) ?
		}
		// set correct port
		frame.Port = "OUT"
		// send into FBP network
		if err := frame.Marshal(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: could not send frame to STDOUT:", err, "- Closing.")
			os.Stdout.Close()
			return
		}
	}
}

//TODO pretty much 1:1 copy from cmd handleCommandOutputRaw() -> reuse?
func handleCommandOutputRaw(debug bool, cout io.Reader) {
	// prepare readers and variables
	bufr := bufio.NewReader(cout)
	buf := make([]byte, bufSize)
	frame := &flowd.Frame{
		Port:     "OUT",
		Type:     "data",
		BodyType: "Data",
	}
	// read loop
	for {
		nbytes, err := bufr.Read(buf)
		if err != nil {
			if err == io.EOF {
				//TODO that is actually normal behavior in mode=each
				fmt.Fprintln(os.Stderr, "WARNING: EOF from command:", err, "- Closing.")
				//cout.Close()
				return
			}
			// other error
			fmt.Fprintln(os.Stderr, "ERROR: reading from command STDOUT:", err, "- Closing.")
			//cout.Close()
			return
		}

		// frame command output and send into FBP network
		frame.Body = buf[0:nbytes]
		if err := frame.Marshal(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: could not send frame to STDOUT:", err, "- Closing.")
			os.Stdout.Close()
			return
		}
	}
}

func copyFrameBodies(cin io.WriteCloser, bufr *bufio.Reader) {
	var frame *flowd.Frame //TODO why is this pointer to Frame?
	var err error
	for {
		// read frame
		frame, err = flowd.ParseFrame(bufr)
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
