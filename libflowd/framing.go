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

type Frame struct {
	Type        string
	BodyType    string
	Port        string
	ContentType string
	Extensions  map[string]string
	Body        *[]byte
}

// NOTE: require bufio.Reader not io.Reader, because textproto.Reader requires one. Making a local one would swallow any following frames into it.
func ParseFrame(stream *bufio.Reader) (f *Frame, err error) {
	// read headers
	textReader := textproto.NewReader(stream) //TODO To avoid denial of service attacks, the provided bufio.Reader should be reading from an io.LimitReader or similar Reader to bound the size of responses.
	header, err := textReader.ReadMIMEHeader()
	if err != nil {
		return nil, errors.New("cannot parse into frame header: " + err.Error())
	}
	if _, ok := header["Type"]; !ok {
		return nil, errors.New("missing Type header field")
	}
	types := strings.SplitN(header.Get("Type"), ".", 2)
	if len(types) != 2 {
		return nil, errors.New("missing separator in Type header field")
	}
	//TODO read any remaining header fields into frame.Extensions
	// NOTE: Port and Content-Type can be missing at the moment
	f = &Frame{Type: types[0], BodyType: types[1], Port: header.Get("Port"), ContentType: header.Get("Content-Type"), Body: nil}

	// read body
	if _, ok := header["Content-Length"]; !ok {
		return nil, errors.New("missing Content-Length header field")
	}
	lenStr := header.Get("Content-Length")
	lenInt, err := strconv.Atoi(lenStr)
	if err != nil {
		return nil, errors.New("converting content length to integer: " + err.Error())
	}
	buf := make([]byte, lenInt)
	if n, err := io.ReadFull(stream, buf); err != nil {
		if err == io.EOF {
			return nil, errors.New("reading full frame body encountered EOF: " + err.Error())
		} else {
			return nil, errors.New(fmt.Sprintf("reading full frame body short read %d bytes of %d expected: %s", n, lenInt, err.Error()))
		}
	}
	f.Body = &buf
	return f, nil
}

func (f *Frame) Marshal(stream io.Writer) error {
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
	if err := printHeaderLine(tpw, "content-type", f.ContentType); err != nil {
		return errors.New("marshal: " + err.Error())
	}
	if err := printHeaderLine(tpw, "content-length", strconv.Itoa(len(*f.Body))); err != nil {
		return errors.New("marshal: " + err.Error())
	}
	//TODO also marshal frame.Extensions header fields
	if err := finalizeHeader(tpw); err != nil {
		return errors.New("marshal: " + err.Error())
	}
	if _, err := bufw.Write(*f.Body); err != nil { //TODO useless conversion
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
