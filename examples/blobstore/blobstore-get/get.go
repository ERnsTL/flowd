package main

import (
	"bufio"
	"fmt"
	"io/ioutil"
	"os"
	"path/filepath"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
)

//const maxFlushWait = 100 * time.Millisecond // flush any buffered outgoing frames after at most this duration

func main() {
	// options
	var (
		datadir string
		careful bool
	)

	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// flush netout after x seconds if there is buffered data
	// NOTE: bufio.Writer.Write() flushes on its own if buffer is full
	/*
		go func() {
			for {
				time.Sleep(maxFlushWait)
				// NOTE: Flush() checks on its own if data buffered
				netout.Flush()
			}
		}()
	*/
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", netin); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		// parse IIP
		opts := strings.Split(iip, ",")
		if len(opts) == 0 || len(opts) < 1 {
			fmt.Fprintln(os.Stderr, "ERROR: no parameters given in IIP, format is [datadir]{,careful}")
			os.Exit(1)
		}
		datadir = opts[0]
		if len(opts) > 1 {
			if opts[1] == "careful" {
				careful = true
				// check for unknown options
				if careful && len(opts) > 2 {
					fmt.Fprintln(os.Stderr, "ERROR: unknown further options in IIP:", opts[2:])
					os.Exit(1)
				}
			} else {
				fmt.Fprintln(os.Stderr, "ERROR: unknown option in IIP:", opts[1])
				os.Exit(1)
			}
		}
		if careful {
			// check for datadir existence and it being a directory and having write permissions
			if err := checkDirectoryCarefullyR(datadir); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: data directory unaccessible:", err)
				os.Exit(1)
			}
		}
	}
	fmt.Fprintln(os.Stderr, "ready")

	// pre-declare to reduce GC allocations
	var (
		inframe  *flowd.Frame
		blobname string
		err      error
	)
	outframe := flowd.Frame{
		Type:     "data",
		BodyType: "Blob",
		Port:     "OUT",
		//ContentType: "application/octet-stream",
		Body: nil,
	}

	// main loop
	for {
		// read IP
		inframe, err = flowd.ParseFrame(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// read blob name from IP/frame body
		blobname = string(inframe.Body)
		if len(blobname) == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: input frame is missing blob name in body. Discarding.")
			continue
		}
		// calculate blob path for storage retrieval
		blobpath := filepath.Join(datadir, blobname)
		// check if blob name tries to escape base datadir
		if careful && !strings.HasPrefix(blobpath, datadir) {
			// report security error
			//TODO maybe report on ERROR outport?
			fmt.Fprintln(os.Stderr, "ERROR: blob name is trying to escape hierarchy:", blobname, "- Discarding.")
			continue
		}
		// get IP/frame body type
		//TODO set this on output frame, may be useful for some type checking?
		// read blob from storage into output frame body
		outframe.Body, err = ioutil.ReadFile(blobpath)
		if err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: could not read blob file", blobpath)
		}
		// write to outport
		if err = outframe.Marshal(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
		}
		if err = netout.Flush(); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: flushing netout:", err)
		}
	}
}

func checkDirectoryCarefullyR(dirpath string) error {
	// check for path existence and it being a directory and having read permissions
	if fileinfo, err := os.Stat(dirpath); err != nil || !fileinfo.IsDir() {
		// report error
		//TODO maybe report on ERROR outport?
		return fmt.Errorf("path does not exist or is not a directory")
	} else if fileinfo.Mode().Perm() >= 666 { // 384 base 10 = 0600 base 8/octal Unix permissions =
		return fmt.Errorf("no read permission to data directory %s: got %d = octal %o", dirpath, fileinfo.Mode().Perm(), fileinfo.Mode().Perm())
	}
	return nil
}
