package dotencoding_exp

import (
	"bufio"
	"bytes"
	"fmt"
	"net/textproto"
	"os"
)

/*
verdict: balances compactness and extendability; is easily debuggable and implementable. easily parseable using regex. lots of existing parsing code - same format as HTTP and mail header.
unique: only format, which will be there until the end of the String data type and all string-based source codes and text files -> predestined for payload format abstraction envelope and header format.
problems: weaker schema (well, duh - thats what makes it universal).
NOTE: must be modified slightly; pure dot-encoding only supports text-based payload -> need to add content-length to support binary payloads.
*/

func main() {
	var b bytes.Buffer
	w := bufio.NewWriter(os.Stdout)
	tpw := textproto.NewWriter(w)
	PrintHeaderLine(tpw, "type", "data.TextMessage")
	PrintHeaderLine(tpw, "content-type", "application/json")
	FinalizeHeader(tpw)
	body := ([]byte)(`{"Name":"Alice","Body":"Hello from Alice","Time":1294706395881547000}`)
	WriteBody(tpw, body)
	w.Flush()
	fmt.Print(b.String())
}

func PrintHeaderLine(w *textproto.Writer, key string, value string) {
	w.PrintfLine("%s: %s", textproto.CanonicalMIMEHeaderKey(key), value)
}

func FinalizeHeader(w *textproto.Writer) {
	w.PrintfLine("")
}

func WriteBody(w *textproto.Writer, body []byte) {
	dw := w.DotWriter()
	dw.Write(body)
	dw.Close()
}
