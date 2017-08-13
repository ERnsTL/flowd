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

//TODO recursive list = /**

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
		if len(opts) == 0 || len(opts) < 1 {
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
			// check for datadir existence and it being a directory and having read/list permissions
			if err := checkDirectoryCarefullyR(datadir); err != nil {
				fmt.Fprintln(os.Stderr, "ERROR: data directory unsuitable:", err)
				os.Exit(1)
			}
		}
	}
	fmt.Fprintln(os.Stderr, "ready")

	// pre-declare to reduce GC allocations
	var (
		inframe  *flowd.Frame
		blobname string
		found    bool
		err      error
	)
	outframe := flowd.Frame{
		Type:     "data",
		BodyType: "BlobListEntry",
		Port:     "OUT",
		//ContentType: "text/plain; charset=utf-8",
		Body: nil,
	}
	bracketO := flowd.BracketOpen("OUT")
	bracketC := flowd.BracketClose("OUT")

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
		// check if blob path tries to escape base datadir
		if careful && !strings.HasPrefix(blobpath, datadir) {
			// report security error
			//TODO maybe report on ERROR outport?
			fmt.Fprintln(os.Stderr, "ERROR: blob name is trying to escape hierarchy:", blobname, "- Discarding.")
			continue
		}
		// list all get IP/frame body type
		//TODO use info for some automatic type checking of value?
		//datatype := inframe.BodyType
		// list all blobs in that directory
		if filelist, err := ioutil.ReadDir(blobpath); err != nil {
			//TODO maybe report on ERROR outport?
			fmt.Fprintf(os.Stderr, "ERROR: unable to list files in path %s: %v\n", blobpath, err)
			continue
		} else {
			// send opening bracket = list begin
			bracketO.Marshal(netout)
			// send list of files downstream
			for _, file := range filelist {
				outframe.Body = []byte(file.Name())
				outframe.Marshal(netout)
			}
			// send closing bracket = list end
			bracketC.Marshal(netout)
		}
	}
}

func checkDirectoryCarefullyR(dirpath string) error {
	// check for path existence and it being a directory and having write permissions
	if dirinfo, err := os.Stat(dirpath); err != nil || !dirinfo.IsDir() {
		// report error
		//TODO maybe report on ERROR outport?
		return fmt.Errorf("path does not exist or is not a directory")
	}
	//TODO maybe checking permissions is enough
	if _, err := ioutil.ReadDir(dirpath); err != nil {
		return fmt.Errorf("no read permission to directory: %s", dirpath)
	}
	return nil
}
