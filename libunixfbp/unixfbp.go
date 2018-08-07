package unixfbp

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"strings"
)

// OpenOutPort opens an output port resp. its named pipe, returns the pipe a buffered writer on it and also stores the entry in OutPorts.
func OpenOutPort(portName string) (netout *bufio.Writer, outPipe *os.File, err error) {
	// check for existence of port
	port, exists := OutPorts[portName]
	if !exists {
		return nil, nil, fmt.Errorf("outport unknown: %s", portName)
	}
	// open named pipe = FIFO
	//outPipe, err = os.OpenFile(port.Path, os.O_RDWR, os.ModeNamedPipe)
	// NOTE: this would allow opening a FIFO for writing without a writer currently present, then blokc on first write
	outPipe, err = os.OpenFile(port.Path, os.O_WRONLY, os.ModeNamedPipe)
	if err != nil {
		return nil, nil, fmt.Errorf("opening outport %s at path %s: %s", portName, port.Path, err)
	}
	// create buffered writer
	netout = bufio.NewWriter(outPipe)
	// return everything, but also keep it here
	port.Pipe = outPipe
	port.Writer = netout
	OutPorts[portName] = port
	return
}

// OpenInPort opens an output port resp. its named pipe, returns the pipe and a buffered reader on it and also stores the entry in OutPorts.
func OpenInPort(portName string) (netin *bufio.Reader, inPipe *os.File, err error) {
	// check for existence of port
	port, exists := InPorts[portName]
	if !exists {
		return nil, nil, fmt.Errorf("inport unknown: %s", portName)
	}
	// open named pipe = FIFO
	// NOTE: this allows opening the FIFO without a writer being present; will return length 0 bytes on read,
	// which is unfortunately interpreted as an error condition or EOF in Go
	//inPipe, err = os.OpenFile(port.Path, os.O_RDONLY|syscall.O_NONBLOCK, os.ModeNamedPipe)
	inPipe, err = os.OpenFile(port.Path, os.O_RDONLY, os.ModeNamedPipe)
	if err != nil {
		return nil, nil, fmt.Errorf("opening inport %s at path %s: %s", portName, port.Path, err)
	}
	// create buffered writer
	netin = bufio.NewReader(inPipe)
	// return everything, but also keep it here
	port.Pipe = inPipe
	port.Reader = netin
	InPorts[portName] = port
	return
}

// ArrayPortMemberNames filters the list of outports to those ports with the given prefix, returning their names.
func ArrayPortMemberNames(prefix string) (members []string) {
	members = make([]string, 2) // assumption (TODO optimize?)
	for name := range OutPorts {
		if strings.HasPrefix(name, prefix) {
			members = append(members, name)
		}
	}
	return
}

// ArrayPortMemberWriters filters the list of outports to those ports with the given prefix, returning their writers.
// NOTE: see also libflowd.SerializeMultiple()
func ArrayPortMemberWriters(prefix string) (members []*bufio.Writer) {
	members = make([]*bufio.Writer, 2) // assumption (TODO optimize?)
	for name, outport := range OutPorts {
		if strings.HasPrefix(name, prefix) {
			members = append(members, outport.Writer)
		}
	}
	return
}

// internal state for the flag parsers for -inport and -inpath as well as -outport and -outpath
var inPortName, outPortName string

type inPortFlag struct{}
type outPortFlag struct{}

// implement flag.Value
func (p inPortFlag) String() string {
	//TODO
	return "inPortFlag.String(): TODO"
}
func (p inPortFlag) Set(value string) error {
	if inPortName == "" {
		// first state; have port name
		inPortName = value
	} else {
		// save entry
		InPorts[inPortName] = InPort{Path: value}
		inPortName = ""
	}
	return nil
}
func (p outPortFlag) String() string {
	//TODO
	return "outPortFlag.String(): TODO"
}
func (p outPortFlag) Set(value string) error {
	if outPortName == "" {
		// first state; have port name
		outPortName = value
	} else {
		// save entry
		OutPorts[outPortName] = OutPort{Path: value}
		outPortName = ""
	}
	return nil
}

// InPort holds the file descriptor and a buffered reader
type InPort struct {
	Path   string
	Pipe   *os.File
	Reader *bufio.Reader
}

// OutPort holds the file descriptor and a buffered reader
type OutPort struct {
	Path   string
	Pipe   *os.File
	Writer *bufio.Writer
}

var (
	// InPorts is the list of input ports
	InPorts = map[string]InPort{}
	// OutPorts is the list of output ports
	OutPorts = map[string]OutPort{}
	// Debug flag value
	Debug bool
	// Quiet flag value
	Quiet bool
)

// DefFlags sets the most common flags for input and output ports as wll as debug and quiet flags
func DefFlags() {
	//InPorts = map[]string{}
	//OutPorts = map[]string{}
	var inportsFlag inPortFlag
	var outportsFlag outPortFlag
	flag.Var(inportsFlag, "inport", "name of an input port (multiple possile); follow up with -infrom")
	flag.Var(inportsFlag, "inpath", "path of named pipe for previously declared input port (multiple possile); precede with -inport")
	flag.Var(outportsFlag, "outport", "name of an output port (multiple possible); follow up with -outto")
	flag.Var(outportsFlag, "outpath", "path of named pipe for previously declared output port (multiple possle); precede with -outport")
	flag.BoolVar(&Debug, "debug", false, "give detailed event output")
	flag.BoolVar(&Quiet, "quiet", false, "no informational output except errors")
}
