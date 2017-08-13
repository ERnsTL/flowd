package flowd_test

import (
	"bufio"
	"bytes"
	"fmt"
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
		Marshal(*bufio.Writer) error
	}
	var _ IWantThisMethod = &flowd.Frame{}
}

func TestFrameParsesAndMarshalsV2(t *testing.T) {
	// parse
	r := bytes.NewBufferString(frameStrV2)
	bufr := bufio.NewReader(r)
	frame, err := flowd.ParseFrame(bufr)
	assert.NoError(t, err, "framing package cannot parse v2 string into frame")
	assert.Equal(t, "data", frame.Type, "parsed wrong data type")
	assert.Equal(t, "TCPPacket", frame.BodyType, "parsed wrong body type")
	assert.Equal(t, "IN", frame.Port, "parsed wrong port")
	//assert.Equal(t, "text/plain", frame.ContentType, "parsed wrong content type")
	assert.Equal(t, "a\n", (string)(frame.Body), "parsed wrong body")

	// marshal
	var buf2 bytes.Buffer
	bufw := bufio.NewWriter(&buf2)
	err = frame.Marshal(bufw)
	assert.NoError(t, err, "frame unmarshal returned error")
	err = bufw.Flush()
	assert.NoError(t, err, "bufio.Writer cannot flush")
	assert.New(t)
	assert.Equal(t, frameStrV2, buf2.String(), "frame cannot marshal itself")
}

func TestFrameParsesAndMarshalsV1(t *testing.T) {
	// parse
	//frameStr := fmt.Sprintf("%s\r\n%s\r\n%s\r\n%s\r\n\r\n%s", "Type: data.TextMessage", "Port: options", "Content-Type: text/plain", "Content-Length: 4", "TEST")
	frameStr := fmt.Sprintf("%s\r\n%s\r\n%s\r\n\r\n%s", "Type: data.TextMessage", "Port: options", "Content-Length: 4", "TEST")
	//r := strings.NewReader(frameStr)
	var frame *flowd.Frame
	var err error
	r := bytes.NewBufferString(frameStr)
	bufr := bufio.NewReader(r)
	frame, err = flowd.ParseFrameV1(bufr)
	assert.NoError(t, err, "framing package cannot parse v1 string into frame")
	assert.Equal(t, "data", frame.Type, "parsed wrong data type")
	assert.Equal(t, "TextMessage", frame.BodyType, "parsed wrong body type")
	assert.Equal(t, "options", frame.Port, "parsed wrong port")
	//assert.Equal(t, "text/plain", frame.ContentType, "parsed wrong content type")
	assert.Equal(t, "TEST", (string)(frame.Body), "parsed wrong body")

	// marshal
	var buf2 bytes.Buffer
	bufw := bufio.NewWriter(&buf2)
	err = frame.MarshalV1(bufw)
	assert.NoError(t, err, "frame unmarshal returned error")
	err = bufw.Flush()
	assert.NoError(t, err, "bufio.Writer cannot flush")
	assert.New(t)
	assert.Equal(t, frameStr, buf2.String(), "frame cannot marshal itself")
}

//TODO test parsing of extension headers

var (
	benchFrame = &flowd.Frame{
		Port:     "IN",
		Type:     "data",
		BodyType: "TCPPacket",
		Extensions: map[string]string{
			"conn-id": "1",
		},
		Body: []byte("a\n"),
	}
	frameStrV1 = fmt.Sprintf("%s\r\n%s\r\n%s\r\n\r\n%s", "Type: data.TCPPacket", "Port: IN", "Content-Length: 2", "a\n")
	frameStrV2 = fmt.Sprintf("2%s\n%s\n%s\n%s\n%s\n\n%s\000", "data", "type:TCPPacket", "port:IN", "conn-id:1", "length:2", "a\n")
)

// save result in package-level variable so that compiler cannot optimize benchmark away
var resultFrame *flowd.Frame

func BenchmarkMarshalV2(b *testing.B) {
	// prepare
	buf := bytes.Buffer{}
	bufw := bufio.NewWriter(&buf)
	var err error
	b.ResetTimer()
	// marshal
	for n := 0; n < b.N; n++ {
		//buf.Reset()
		err = benchFrame.Marshal(bufw)
		if err != nil {
			b.Errorf("MarshalV2 returned error: %s", err)
		}
		/*
			err = bufw.Flush()
			if err != nil {
				b.Errorf("could not flush frame into buffer: %s", err)
			}
		*/
	}
}

func BenchmarkMarshalV1(b *testing.B) {
	// prepare
	buf := bytes.Buffer{}
	bufw := bufio.NewWriter(&buf)
	var err error
	b.ResetTimer()
	// marshal
	for n := 0; n < b.N; n++ {
		//buf.Reset()
		err = benchFrame.MarshalV1(bufw)
		if err != nil {
			b.Errorf("MarshalV1 returned error: %s", err)
		}
		/*
			err = bufw.Flush()
			if err != nil {
				b.Errorf("could not flush frame into buffer: %s", err)
			}
		*/
	}
}

func BenchmarkParseFrameV2(b *testing.B) {
	// prepare input buffer
	r := bytes.NewBufferString(frameStrV2)
	for n := 0; n < b.N-1; n++ {
		r.WriteString(frameStrV2)
	}
	bufr := bufio.NewReader(r)
	var frame *flowd.Frame
	var err error
	b.ResetTimer()
	// parse it all
	for n := 0; n < b.N; n++ {
		frame, err = flowd.ParseFrame(bufr)
		if err != nil {
			b.Errorf("ParseFrameV2 returned error: %s", err)
		}
		resultFrame = frame
	}
}

func BenchmarkParseFrameV1(b *testing.B) {
	// prepare input buffer
	r := bytes.NewBufferString(frameStrV1)
	for n := 0; n < b.N-1; n++ {
		r.WriteString(frameStrV1)
	}
	bufr := bufio.NewReader(r)
	var frame *flowd.Frame
	var err error
	b.ResetTimer()
	// parse it all
	for n := 0; n < b.N; n++ {
		frame, err = flowd.ParseFrameV1(bufr)
		if err != nil {
			b.Errorf("ParseFrameV1 returned error: %s", err)
		}
		resultFrame = frame
	}
}
