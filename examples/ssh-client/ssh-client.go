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
	"os/exec"
	"strings"
	"time"

	"github.com/kballard/go-shellquote"
	"golang.org/x/crypto/ssh"
	sshagent "golang.org/x/crypto/ssh/agent"

	flowd "github.com/ERnsTL/flowd/libflowd"
)

const bufSize = 65536

var (
	debug bool
	quiet bool
)

func main() {
	// open connection to network
	stdin := bufio.NewReader(os.Stdin)
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	iip, err := flowd.GetIIP("CONF", stdin)
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- exiting.")
		os.Exit(1)
	}
	// parse IIP
	var retry, bridge, useAgent bool
	var pkPath, khPath, hostkeyLine string
	var operatingMode OperatingMode
	iipSplit, err := shellquote.Split(iip)
	checkError(err)
	flags := flag.NewFlagSet("ssh-client", flag.ContinueOnError)
	flags.Var(&operatingMode, "mode", "operating mode: one (command instance handling all IPs) or each (IP handled by new instance)")
	flags.BoolVar(&useAgent, "agent", true, "use ssh-agent as key source")
	flags.StringVar(&pkPath, "i", "", "use private key file, e.g. ~/.ssh/id_ed25519|ecdsa|rsa|dsa")
	flags.StringVar(&khPath, "knownhosts", os.ExpandEnv("$HOME/.ssh/known_hosts"), "path to SSH known_hosts file containing known host keys, may select wrong algo if multiple")
	flags.StringVar(&hostkeyLine, "hostkey", "", "use hostkey line for server verification, put in quotes, get it using ssh-keyscan or from /etc/ssh/ssh_host_*_key.pub, must be for algo actually used")
	flags.BoolVar(&bridge, "bridge", true, "bridge mode, true = forward frames from/to FBP network, false = send frame body over SSH, frame data from SSH")
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
	cmdArgs := flags.Args()[1:]
	if debug {
		fmt.Fprintf(os.Stderr, "cmdArgs=%v\n", cmdArgs)
	}

	//TODO implement
	if retry {
		fmt.Fprintln(os.Stderr, "ERROR: flag -retry currently unimplemented - exiting.")
		os.Exit(2)
	}

	// parse remote address as URL
	//TODO parse something like ssh://user@sampleserver.com
	// NOTE: add double slashes after semicolon
	remoteURLStr := flags.Args()[0]
	if !strings.HasPrefix(remoteURLStr, "ssh://") {
		remoteURLStr = "ssh://" + remoteURLStr
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
				fmt.Fprintf(os.Stderr, "hostkey callback shall check %s\n", hostname)
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
				fmt.Fprintf(os.Stderr, "found host key, checking...\n")
			}
			if !bytes.Equal(key.Marshal(), foundHostKey.Marshal()) {
				return fmt.Errorf("known host key found for active key type, but mismatch")
			}

			if !quiet {
				fmt.Fprintf(os.Stderr, "host key verified\n")
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
	connServer, err := ssh.Dial("tcp", remoteHost, sshConfig)
	checkError(err)
	defer connServer.Close()
	//TODO unreachable defer agentConn.Close()
	if !quiet {
		fmt.Fprintln(os.Stderr, "connected")
	}

	// make session for remote command
	// NOTE: each session accepts only one call to Run, Start, Shell, Output or CombinedOutput
	started := time.Now()

	if !quiet {
		fmt.Fprintln(os.Stderr, "asking for new session")
	}
	session, err := connServer.NewSession()
	checkError(err)
	defer session.Close()
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

	// prepare for exec request
	cmdOut := new(bytes.Buffer)
	cmdErr := new(bytes.Buffer)
	session.Stdout = cmdOut
	session.Stderr = cmdErr
	if !quiet {
		fmt.Fprintf(os.Stderr, "starting remote command %v\n", cmdArgs)
	}
	// NOTE: Run = Start + Wait
	// TODO optimize - first split in IIP and flags, then joined here
	cmdArgsStr := strings.Join(cmdArgs, " ")
	err = session.Run(cmdArgsStr)
	runtime := time.Now().Sub(started).Seconds()
	if !quiet {
		fmt.Fprintf(os.Stderr, "runtime of request was %.03f sec\n", runtime)
	}
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
		default:
			// TODO check detailed for IO errors, check ssh package source code
			fmt.Fprintf(os.Stderr, "other error occured: %v\n", err)
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

	// print output
	if debug {
		fmt.Fprintf(os.Stderr, "program STDOUT result: %s\n", cmdOut.String())
		fmt.Fprintf(os.Stderr, "program STDERR result: %s\n", cmdErr.String())
	}

	if debug {
		stats := map[string]string{
			"stdout":  fmt.Sprintf("%d bytes", len(cmdOut.String())),
			"stderr":  fmt.Sprintf("%d bytes", len(cmdErr.String())),
			"runtime": fmt.Sprintf("%0.3f", runtime),
			"status":  fmt.Sprintf("%d", exitCode),
		}
		fmt.Fprintf(os.Stderr, "%+v\n", stats)
	}

	fmt.Fprintln(os.Stderr, "done with SSH call")

	//--- end SSH client

	// prepare remote subprocess variables
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
		cin, cout = startCommand(cmdargs)
		defer cout.Close()
		defer cin.Close()

		// handle subprocess output
		go handleCommandOutput(debug, cout)

		// handle subprocess input
		if bridge == true {
			// setup direct copy without processing (since already framed)
			if _, err := io.Copy(cin, bufr); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: receiving from FBP network:", err, "Closing.")
				os.Stdin.Close()
				return
			}
		} else {
			// loop: read frame, write body to subprocess
			copyFrameBodies(cin, bufr)
		}
	case Each:
		// prepare variables
		var frame *flowd.Frame //TODO why is this pointer to Frame?
		var err error

		for {
			// read frame
			frame, err = flowd.ParseFrame(bufr)
			if err != nil {
				fmt.Fprintln(os.Stderr, err)
			} else if debug {
				fmt.Fprintln(os.Stderr, "received frame:", string(frame.Body))
			}

			// handle each frame with a new instance
			go func(frame *flowd.Frame) {
				// start new command instance
				cmd, cin, cout = startCommand(cmdargs)

				// handle subprocess output
				if framing {
					go handleCommandOutput(debug, cout)
				} else {
					go handleCommandOutputRaw(debug, cout)
				}

				// forward frame or frame body to subprocess
				if framing {
					// frame
					if err := frame.Marshal(cin); err != nil {
						fmt.Fprintln(os.Stderr, "ERROR: marshaling frame to command STDIN:", err, "- Exiting.")
						os.Exit(3)
					}
				} else {
					// frame body
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

func checkError(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "ERROR:", err)
		os.Exit(2)
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "IIP format: [flags] [ssh://][user=me]:[password]@[host][:port=22] [cmdpath] [args...]")
}

//TODO pretty much 1:1 copy from cmd startCommand() -> reuse?
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

//TODO pretty much 1:1 copy from cmd handleCommandOutput() -> reuse?
func handleCommandOutput(debug bool, cout io.ReadCloser) {
	bufr := bufio.NewReader(cout)
	for {
		frame, err := flowd.ParseFrame(bufr)
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
		if debug == true {
			fmt.Fprintln(os.Stderr, "STDOUT received frame type", frame.Type, "and data type", frame.BodyType, "for port", frame.Port, "with body:", (string)(frame.Body)) //TODO what is difference between this and string(frame.Body) ?
		}
		// set correct port
		frame.Port = "OUT"
		// send into FBP network
		if err := frame.Marshal(os.Stdout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: could not send frame to STDOUT:", err, "- Closing.")
			os.Stdout.Close()
			return
		}
	}
}

//TODO pretty much 1:1 copy from cmd handleCommandOutputRaw() -> reuse?
func handleCommandOutputRaw(debug bool, cout io.ReadCloser) {
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
		if err := frame.Marshal(os.Stdout); err != nil {
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
