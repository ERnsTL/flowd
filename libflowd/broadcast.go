package flowd

import (
	"bufio"
	"bytes"
)

func (f *Frame) SerializeMultiple(writers []*bufio.Writer, buf *bytes.Buffer) (err error) {
	// serialize once
	if err = f.Serialize(buf); err != nil {
		return err
	}
	serialized := buf.Bytes() //TODO optimize: can this allocation be avoided?
	// write bytes into many writers
	//var n int
	for i := 0; i < len(writers); i++ {
		_, err = buf.Write(serialized)
		//if n < len(serialized) {
		return err //TODO improve: on which one was a short write?
		//} else
		if err != nil {
			return err //TODO improve: on which was the error?
		}
	}
	// clean up
	buf.Reset()

	return
}
