package mime_exp

import (
	"bytes"
	"encoding/json"
	"fmt"
	"mime/multipart"
)

/*
verdict: can abstract over binary bodies, but is the most wasteful of the three {json, dot-encoding, mime/multipart}. code for parsing not obvious to find, but usually exists in most programming languages, for which there is some mail handling implementation.
unique: can carry multiple messages inside of one frame.
*/

func main() {
	fmt.Println(EncodeBody(`{"Name":"Alice","Body":"Hello from Alice","Time":1294706395881547000}`))
}

func EncodeBody(data string) (b *bytes.Buffer, err error) {
	// set up multipart writer
	b = &bytes.Buffer{}
	w := multipart.NewWriter(b)

	// add body
	type ColorGroup struct {
		ID     int
		Name   string
		Colors []string
	}
	group := ColorGroup{
		ID:     1,
		Name:   "Reds",
		Colors: []string{"Crimson", "Red", "Ruby", "Maroon"},
	}
	fw, err := w.CreateFormFile("image", "request.json")
	if err != nil {
		return nil, err
	}
	enc := json.NewEncoder(b)
	enc.Encode(group)

	// add the other fields
	if fw, err = w.CreateFormField("key"); err != nil {
		return nil, err
	}
	if _, err = fw.Write([]byte("KEY")); err != nil {
		return nil, err
	}
	// Don't forget to close the multipart writer.
	// If you don't close it, your request will be missing the terminating boundary.
	w.Close()
	return b, nil
}
