package flowd

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"net/textproto"
	"strconv"
	"strings"
)

// Frame is a central structure used for carrying information through the processing network
type Frame struct {
	Type       string
	BodyType   string
	Port       string
	Extensions map[string]string
	Body       []byte
}

/*
v1 format based on MIME:

Type: data.MessageTypeRN
Port: OUTRN
Content-Length: 10RN
Conn-Id: 123RN
RN
= 71 bytes

v2 format based on STOMP v1.2:

2dataN
type:MessageTypeN
port:OUTN
length:10N
conn-id:123N
N
= 54 bytes
*/

const maxBodySize = 1 * 1000 * 1000 * 1000 // 1 GByte

// ParseFrame reads an IP from a buffered data stream, like STDOUT from a network process or a network connection
/*
The frame format is that of STOMP v1.2, with the following modifications:
* content-length header field is renamed length
* length header field must be present if body is present
* if length header is absent, no scanning until null byte is done, but absent body is assumed
* content-type is optional and is no special header field, but may be used by components and application networks
* TODO no escape substitution in header values - for now at least
* TODO no support for multiple occurrences of same header field; gets overwritten -> last value remains
* all header field names are to be in lowercase
* COMMAND line contains not a command, but the frame type, also in lowercase, e.g. data, control
* the port and type header fields have special meaning; type = body type, port = input / output port name
* before COMMAND, a single byte is read (version marker) to designate the frame format version
* this format has designation '2' = 0x32
*/
//TODO want re-use err -> but then have to var-define all other return values -> ugly -> benchmark
func ParseFrame(stream *bufio.Reader) (f *Frame, err error) {
	// read version marker
	version, err := stream.ReadByte()
	if err != nil {
		// first read is usual place to block for new frame, so EOF is no error here
		if err == io.EOF {
			return nil, err
		}
		return nil, errors.New("reading version marker: " + err.Error())
	}
	switch version {
	case '2':
		// OK, will do it here
	case '1':
		// old format
		return ParseFrameV1(stream)
	default:
		// unknown version
		return nil, errors.New("unknown version marker: " + string(version))
	}
	/* TODO
	if version != '2' {
		//TODO
	}
	*/

	// initialize frame if need be
	if f == nil {
		f = &Frame{}
	}

	// read STOMP command line = frame type
	frameType, _, err := stream.ReadLine()
	if err != nil {
		return nil, errors.New("reading frame type: " + err.Error())
	}
	f.Type = string(frameType)

	// read header
	var bodyLength int
	line, _, err := stream.ReadLine()
	if err != nil {
		return nil, errors.New("reading header line: " + err.Error())
	}
	var kv []string
	// read until empty line = end of header
	for len(line) > 0 {
		// split line on :
		kv = strings.SplitN(string(line), ":", 2)
		if err != nil {
			return nil, errors.New("splitting header line: " + err.Error())
		}
		if len(kv) != 2 {
			return nil, errors.New("parts in header line != 2: " + strconv.Itoa(len(kv)))
		}

		// store line appropriately
		switch kv[0] {
		case "port":
			f.Port = kv[1]
		case "length":
			bodyLength, err = strconv.Atoi(kv[1])
			if err != nil {
				return nil, errors.New("parsing body length value: " + err.Error())
			}
			if bodyLength < 0 || bodyLength > maxBodySize {
				return nil, errors.New("given body length out of bounds: " + strconv.Itoa(bodyLength))
			}
		case "type":
			f.BodyType = kv[1]
		default:
			// all other fields ("extensions")
			if f.Extensions == nil {
				f.Extensions = map[string]string{}
			}
			f.Extensions[kv[0]] = kv[1]
		}

		// try to read next header line
		line, _, err = stream.ReadLine()
		if err != nil {
			return nil, errors.New("reading next header line: " + err.Error())
		}
	}

	// read body
	if bodyLength > 0 {
		buf := make([]byte, bodyLength)
		if _, err = io.ReadFull(stream, buf); err != nil {
			if err == io.EOF {
				return nil, errors.New("reading full frame body hit EOF: " + err.Error())
			}
			return nil, errors.New("reading full frame body short read: " + err.Error())
		}
		f.Body = buf
	}

	// read frame terminator octet = \0
	nul, err := stream.ReadByte()
	if err != nil {
		return nil, errors.New("reading frame terminator: " + err.Error())
	}
	if nul != 0x00 {
		return nil, errors.New("frame terminator is not 0x00: " + string(nul))
	}

	return f, nil
}

// ParseFrameV1 parses a frame in v1 format = strict MIME header
// NOTE: require bufio.Reader not io.Reader, because textproto.Reader requires one. Making a local one would swallow any following frames into it.
func ParseFrameV1(stream *bufio.Reader) (f *Frame, err error) {
	// read headers
	textReader := textproto.NewReader(stream) //TODO To avoid denial of service attacks, the provided bufio.Reader should be reading from an io.LimitReader or similar Reader to bound the size of responses.
	header, err := textReader.ReadMIMEHeader()
	var port []string
	var ipType []string
	var found bool
	if err != nil {
		if err == io.EOF {
			// just an EOF received, return it as such
			return nil, err
		}
		return nil, errors.New("cannot parse into frame header: " + err.Error())
	}
	if ipType, found = header["Type"]; !found {
		return nil, errors.New("missing Type header field")
	}
	if port, found = header["Port"]; !found {
		return nil, errors.New("missing Port header field")
	}
	types := strings.SplitN(ipType[0], ".", 2)
	if len(types) != 2 {
		return nil, errors.New("missing separator in Type header field")
	}
	// initialize frame structure
	f = &Frame{Type: types[0], BodyType: types[1], Port: port[0], Body: nil}
	// read content length
	var lenStr []string
	if lenStr, found = header["Content-Length"]; !found {
		return nil, errors.New("missing Content-Length header field")
	}
	lenInt, err := strconv.Atoi(lenStr[0])
	if err != nil {
		return nil, errors.New("converting content length to integer: " + err.Error())
	}
	// read any remaining header fields into frame.Extensions
	//FIXME optimize: do without the deletions
	//TODO decide if map[string]string suffices (are duplicate headers useful? maybe for layered information.)
	if len(header) > 3 {
		delete(header, "Type")
		delete(header, "Port")
		delete(header, "Content-Length")
		f.Extensions = make(map[string]string)
		for key, values := range header {
			//FIXME implement this correctly
			f.Extensions[key] = values[0]
			//fmt.Fprintf(os.Stderr, "framing got extension header %s = %s\n", key, values[0])
		}
	}
	// read body
	buf := make([]byte, lenInt)
	if n, err := io.ReadFull(stream, buf); err != nil {
		if err == io.EOF {
			return nil, errors.New("reading full frame body encountered EOF: " + err.Error())
		}
		return nil, fmt.Errorf("reading full frame body short read %d bytes of %d expected: %s", n, lenInt, err.Error())
	}
	f.Body = buf
	return f, nil
}

// Marshal serializes an IP into a data stream, like STDIN into a network process or a network connection
//TODO optimize: does Go pre-calculate all values like []byte{'2'} ?
//TODO optimize: is +"\n" efficient?
func (f *Frame) Marshal(stream io.Writer) (err error) {
	// write version marker
	_, err = stream.Write([]byte{'2'})
	if err != nil {
		return errors.New("writing version marker: " + err.Error())
	}

	// write frame type
	if f.Type == "" {
		return errors.New("type is empty")
	}
	// NOTE: strings.ToUpper() increases runtime by 7 %
	_, err = stream.Write([]byte(f.Type + "\n"))
	if err != nil {
		return errors.New("writing frame type: " + err.Error())
	}

	// write body type, if present
	if f.BodyType != "" {
		_, err = stream.Write([]byte("type:" + f.BodyType + "\n"))
		if err != nil {
			return errors.New("writing body type: " + err.Error())
		}
	}

	// write port, if present
	if f.Port != "" {
		_, err = stream.Write([]byte("port:" + f.Port + "\n"))
		if err != nil {
			return errors.New("writing port: " + err.Error())
		}
	}

	// write any other header fields, if present
	if f.Extensions != nil {
		for key, value := range f.Extensions {
			// write line
			// NOTE: strings.ToLower() increases runtime by 10 %
			_, err = stream.Write([]byte(key + ":" + value + "\n"))
			if err != nil {
				return errors.New("writing extension header lines: " + err.Error())
			}
		}
	}

	// if body present, write length
	if f.Body != nil {
		_, err = stream.Write([]byte("length:" + strconv.Itoa(len(f.Body)) + "\n"))
		if err != nil {
			return errors.New("writing body length: " + err.Error())
		}

		// write end-of-header marker = empty line
		_, err = stream.Write([]byte{'\n'})
		if err != nil {
			return errors.New("writing end-of-header marker: " + err.Error())
		}

		// write body
		_, err = stream.Write(f.Body)
		if err != nil {
			return errors.New("writing body: " + err.Error())
		}
	} else {
		// write only end-of-header marker = empty line
		_, err = stream.Write([]byte{'\n'})
		if err != nil {
			return errors.New("writing end-of-header marker: " + err.Error())
		}
	}

	// write frame terminator null byte
	_, err = stream.Write([]byte{0x00})
	if err != nil {
		return errors.New("writing frame terminator: " + err.Error())
	}

	// success
	return nil
}

// MarshalV1 serializes an IP into a data stream in previous format (strict MIME + content-length)
//TODO avoid allocating buffered writer on every call
func (f *Frame) MarshalV1(stream io.Writer) error {
	if f == nil {
		return errors.New("refusing to marshal nil frame")
	}
	bufw := bufio.NewWriter(stream)
	tpw := textproto.NewWriter(bufw)
	if err := printHeaderLine(tpw, "type", f.Type+"."+f.BodyType); err != nil {
		return errors.New("marshal: " + err.Error())
	}
	if err := printHeaderLine(tpw, "port", f.Port); err != nil {
		return errors.New("marshal: " + err.Error())
	}
	if err := printHeaderLine(tpw, "content-length", strconv.Itoa(len(f.Body))); err != nil {
		return errors.New("marshal: " + err.Error())
	}
	if f.Extensions != nil {
		for key, value := range f.Extensions {
			if err := printHeaderLine(tpw, key, value); err != nil {
				return errors.New("marshal extension header: " + err.Error())
			}
			//fmt.Fprintf(os.Stderr, "marshal extension header: %s = %s\n", key, value)
		}
	}
	if err := finalizeHeader(tpw); err != nil {
		return errors.New("marshal: " + err.Error())
	}
	if _, err := bufw.Write(f.Body); err != nil {
		return errors.New("marshal: writing body: " + err.Error())
	}
	if err := bufw.Flush(); err != nil {
		return errors.New("marshal: flushing writer: " + err.Error())
	}
	return nil
}

func printHeaderLine(w *textproto.Writer, key string, value string) error {
	if err := w.PrintfLine("%s: %s", textproto.CanonicalMIMEHeaderKey(key), value); err != nil {
		return errors.New("writing header line: " + err.Error())
	}
	return nil
}

func finalizeHeader(w *textproto.Writer) error {
	if err := w.PrintfLine(""); err != nil {
		return errors.New("finalizing header: " + err.Error())
	}
	return nil
}
