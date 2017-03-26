package flowd_test

import (
	"bufio"
	"bytes"
	"fmt"
	"io"
	"testing"

	"github.com/ERnsTL/flowd/libflowd"
	"github.com/stretchr/testify/assert"
)

/*
func TestConfigMalformed(t *testing.T) {
	var err error
	for i, config := range nokconfigs {
		_, err = GetAllNews(config)
		assert.Error(t, err, "config malformed test entry #%d", i)
	}
}
*/
// TODO change to methods and Decoder, Encoder like https://golang.org/pkg/encoding/#TextMarshaler

/*
func TestHasFunctionParseFrame(t *testing.T) {
	flowd.ParseFrame(nil)
}
*/

func TestHasStructFrame(t *testing.T) {
	var _ flowd.Frame
}

func TestFrameHasRequiredMethods(t *testing.T) {
	type IWantThisMethod interface {
		Marshal(io.Writer) error
	}
	var _ IWantThisMethod = &flowd.Frame{}
}

func TestFrameParsesAndMarshals(t *testing.T) {
	// parse
	//frameStr := fmt.Sprintf("%s\r\n%s\r\n%s\r\n%s\r\n\r\n%s", "Type: data.TextMessage", "Port: options", "Content-Type: text/plain", "Content-Length: 4", "TEST")
	frameStr := fmt.Sprintf("%s\r\n%s\r\n%s\r\n\r\n%s", "Type: data.TextMessage", "Port: options", "Content-Length: 4", "TEST")
	//r := strings.NewReader(frameStr)
	var frame *flowd.Frame
	var err error
	r := bytes.NewBufferString(frameStr)
	bufr := bufio.NewReader(r)
	frame, err = flowd.ParseFrame(bufr)
	assert.NoError(t, err, "framing package cannot parse string into frame")
	assert.Equal(t, "data", frame.Type, "parsed wrong data type")
	assert.Equal(t, "TextMessage", frame.BodyType, "parsed wrong body type")
	assert.Equal(t, "options", frame.Port, "parsed wrong port")
	//assert.Equal(t, "text/plain", frame.ContentType, "parsed wrong content type")
	assert.Equal(t, "TEST", (string)(frame.Body), "parsed wrong body")

	// marshal
	var buf2 bytes.Buffer
	bufw := bufio.NewWriter(&buf2)
	err = frame.Marshal(bufw)
	assert.NoError(t, err, "frame unmarshal returned error")
	assert.New(t)
	assert.Equal(t, frameStr, buf2.String(), "frame cannot marshal itself")
}

//TODO test parsing of extension headers
