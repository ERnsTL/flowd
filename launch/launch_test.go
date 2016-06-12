package main

import (
	"net/url"
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestHasStructFrame(t *testing.T) {
	var _ endpoint
}

func TestEndpointHasRequiredFields(t *testing.T) {
	_ = endpoint{Url: &url.URL{Scheme: "udp", Host: "localhost:0"}}
}

var nokendpoints = []string{
	"malformed",
	"udp6://localhost:70000",
	"udp4://localhost:65536",
	"udp://localhost:-1",
	"udp:localhost:",
	"udp:localhost",
	"udp:localhost:0",
	"tcp://localhost:0",
	"tcp4://localhost:0",
	"tcp6://localhost:0",
	"udp::0",
	"udp::10000",
	"udp4::0",
	"udp4::10000",
	"udp6::0",
	"udp6::10000",
	"udp://localhost:0/",
	"udp://localhost:0/foo/",
	"udp://localhost:0#foo",
	"udp://localhost:0/#foo",
	"udp://localhost:0/#foo?bar",
	"udp://localhost:0?foo",
	"udp://localhost:0/?foo",

	// localhost
	"udp6://::1:0",
	"udp6://::1:4000",
	"udp6://::1:40000",
	"udp6://::1:65535",
	// all interfaces
	"udp6://:::0",
	"udp6://:::4000",
	"udp6://:::40000",
	"udp6://:::65535",
}

var okendpoints = []string{
	"udp://localhost:",
	"udp://:0",
	"udp4://:0",
	"udp6://:0",

	"udp://localhost:0",
	"udp4://localhost:0",
	"udp6://localhost:0",
	"udp://localhost:4000",
	"udp4://localhost:4000",
	"udp6://localhost:4000",
	"udp://localhost:40000",
	"udp4://localhost:40000",
	"udp6://localhost:40000",
	"udp://localhost:65535",
	"udp4://localhost:65535",
	"udp6://localhost:65535",

	"udp://127.0.0.1:0",
	"udp4://127.0.0.1:0",
	"udp://127.0.0.1:4000",
	"udp4://127.0.0.1:4000",
	"udp://127.0.0.1:40000",
	"udp4://127.0.0.1:40000",
	"udp://127.0.0.1:65535",
	"udp4://127.0.0.1:65535",

	"udp://0.0.0.0:0",
	"udp4://0.0.0.0:0",
	"udp://0.0.0.0:4000",
	"udp4://0.0.0.0:4000",
	"udp://0.0.0.0:40000",
	"udp4://0.0.0.0:40000",
	"udp://0.0.0.0:65535",
	"udp4://0.0.0.0:65535",
}

func TestEndpointRejectsUncorrectEndpoints(t *testing.T) {
	var e inputEndpoints
	var err error
	for _, estr := range nokendpoints {
		err = e.Set(estr)
		assert.Error(t, err, "endpoint malformed test entry %s", estr)
	}
}

func TestEndpointAcceptsCorrectEndpoints(t *testing.T) {
	var e inputEndpoints
	var err error
	for _, estr := range okendpoints {
		err = e.Set(estr)
		assert.NoError(t, err, "endpoint well-formed test entry %s", estr)
	}
}

//TODO update for no UDP, +unix and +unixpacket
//TODO test cmdline flag parsing
//TODO test port name parsing including array ports
//TODO test missing component name
//TODO test for unexistent component file name

//TODO test for unresolvable address in endpoints
//TODO test actually dialing in and out using mock UDPConn

//TODO test data flow using passthrough component

//TODO test if discoverable not before, but during and not anymore after (correct cleanup)
//TODO test if port number is printed on stdout
