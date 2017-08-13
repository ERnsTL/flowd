package main

import (
	"bufio"
	"fmt"
	"io/ioutil"
	"log"
	"os"
	"path/filepath"
	"strings"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/fsnotify/fsnotify"
)

//FIXME unfinished program - not sure how this might get used
/*TODO helpful:
  https://noypi-linux.blogspot.co.at/2014/07/file-monitoring-with-golang_4.html
  https://github.com/noypi/filemon
  https://github.com/fsnotify/fsnotify
  https://github.com/fsnotify/fsnotify/blob/master/example_test.go
*/

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
	var inframe *flowd.Frame
	var err error
	outframe := flowd.Frame{
		Type:     "data",
		BodyType: "BlobChanged",
		Port:     "OUT",
		//ContentType: "text/plain; charset=utf-8",
		Body: nil,
	}
	var blobname string

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
		watcher, err := fsnotify.NewWatcher()
		if err != nil {
			log.Fatal(err)
		}
		defer watcher.Close()

		done := make(chan bool)
		go func() {
			for {
				select {
				case event := <-watcher.Events:
					log.Println("event:", event)
					if event.Op&fsnotify.Write == fsnotify.Write {
						log.Println("modified file:", event.Name)
					}
				case err = <-watcher.Errors:
					log.Println("ERROR:", err)
				}
			}
		}()

		err = watcher.Add("/tmp/foo")
		if err != nil {
			log.Fatal(err)
		}
		<-done

		//TODO set this on output frame, may be useful for some type checking?
		// read blob from storage into output frame body
		outframe.Body, err = ioutil.ReadFile(blobpath)
		if err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: could not read blob file", blobpath)
		}
		// write to outport
		if err := outframe.Marshal(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
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
