package main

import (
	"bufio"
	"fmt"
	"os"
	"sync"
	"time"

	"github.com/ERnsTL/flowd/libflowd"
)

const maxFlushWait = 2000 * time.Millisecond // flush any buffered outgoing frames after at most this duration

func main() {
	// connect to network
	netin := bufio.NewReader(os.Stdin)
	netout := bufio.NewWriter(os.Stdout)
	defer netout.Flush()
	// flush netout after x seconds if there is buffered data
	var netoutLock sync.Mutex // bufio.Writer is not concurrency-safe, thus needs a lock
	go func() {
		for {
			time.Sleep(maxFlushWait)
			netoutLock.Lock()
			// NOTE: bufio.Writer.Write() flushes on its own if buffer is full
			// NOTE: Flush() checks on its own if data buffered
			netout.Flush()
			netoutLock.Unlock()
		}
	}()

	// get configuration from IIP = initial information packet/frame
	var delay time.Duration
	fmt.Fprintln(os.Stderr, "wait for IIP")
	if iip, err := flowd.GetIIP("CONF", netin); err != nil {
		fmt.Fprintln(os.Stderr, "ERROR getting IIP:", err, "- Exiting.")
		os.Exit(1)
	} else {
		if len(iip) == 0 {
			fmt.Fprintln(os.Stderr, "ERROR: no delay time given in IIP, format is [duration]")
			os.Exit(1)
		} else if delay, err = time.ParseDuration(iip); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: malformed delay time given in IIP, format is [duration], eg. 5s")
			os.Exit(1)
		}
	}
	fmt.Fprintln(os.Stderr, "got delay time", delay)

	var frame *flowd.Frame //TODO why is this pointer to frame?
	var err error

	for {
		// read frame
		frame, err = flowd.ParseFrame(netin)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
		}

		// sleep
		fmt.Fprintln(os.Stderr, "got frame, delaying...")
		time.Sleep(delay)
		fmt.Fprintln(os.Stderr, "now forwarding.")

		// send it to given output ports
		frame.Port = "OUT"
		netoutLock.Lock()
		if err = frame.Marshal(netout); err != nil {
			fmt.Fprintln(os.Stderr, "ERROR: marshaling frame:", err.Error())
		}
		netoutLock.Unlock()
	}
}
