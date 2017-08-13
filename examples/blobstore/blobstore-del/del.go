package main

import (
	"bufio"
	"fmt"
	"io/ioutil"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
)

func main() {
	// parameters
	var (
		datadir string
		careful bool
	)

	// open connection to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// get configuration from IIP = initial information packet/frame
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", netin); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		// parse IIP
		opts := strings.Split(iip, ",")
		if len(opts) == 0 || len(opts) < 2 {
			fmt.Fprintln(os.Stderr, "ERROR: no parameters given in IIP, format is [datadir]{,careful}")
			os.Exit(1)
		}
		datadir = opts[0]
		if len(opts) > 1 {
			if opts[1] == "careful" {
				careful = true
				// check for unknown options
				if careful && len(opts) > 3 {
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
			if err := checkDirectoryCarefullyW(datadir); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: data directory unsuitable:", err)
				os.Exit(1)
			}
		}
	}
	fmt.Fprintln(os.Stderr, "ready")

	// pre-declare to reduce GC allocations
	var (
		inframe  *flowd.Frame
		err      error
		blobname string
		found    bool
	)

	// main loop
	for {
		// read IP
		inframe, err = flowd.ParseFrame(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// check for header Blob-Name and get blobname
		if blobname, found = inframe.Extensions["Blob-Name"]; !found {
			fmt.Fprintln(os.Stderr, "ERROR: input frame is missing Blob-Name header field. Discarding.")
			continue
		}
		// calculate blob path for storage
		blobpath := filepath.Join(datadir, blobname)
		// check if blob name tries to escape base datadir
		if careful && !strings.HasPrefix(blobpath, datadir) {
			// report security error
			//TODO maybe report on ERROR outport?
			fmt.Fprintln(os.Stderr, "ERROR: blob name is trying to escape hierarchy:", blobname, "- Discarding.")
			continue
		}
		// get IP/frame body type
		//TODO use info for some automatic type checking of value?
		//datatype := inframe.BodyType
		// delete file
		if err := os.Remove(blobpath); err != nil {
			//TODO send error downstream?
			fmt.Fprintln(os.Stderr, "ERROR: could not delete blob", blobpath)
		} else {
			//TODO send success downstream?
		}
	}
}

func checkDirectoryCarefullyW(dirpath string) error {
	// check for path existence and it being a directory and having write permissions
	testfilepath := filepath.Join(dirpath, "test"+strconv.Itoa(os.Getpid()))
	if fileinfo, err := os.Stat(dirpath); err != nil || !fileinfo.IsDir() {
		// report error
		//TODO maybe report on ERROR outport?
		return fmt.Errorf("path does not exist or is not a directory")
	} else if err := ioutil.WriteFile(testfilepath, nil, 0640); err != nil {
		return fmt.Errorf("no write permission to directory: %s", dirpath)
	} else {
		// delete test file
		if err := os.Remove(testfilepath); err != nil {
			return fmt.Errorf("could not remove/delete test file: %s", testfilepath)
		}
	}
	return nil
}
