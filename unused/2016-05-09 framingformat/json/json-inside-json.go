package json_exp

import (
	"encoding/json"
	"fmt"
	"log"
)

type Message struct {
	Type      string `json:type`
	Timestamp string `json:timestamp`
	Data      json.RawMessage	// NOTE: this is the crucial part. this Data is then JSON-parsed according to Type attribute.
}

type Event struct {
	Type    string `json:type`
	Creator string `json:creator`
}

var data = []byte(`{
  "type": "event",
  "data": {
       "type": "monitoring",
       "creator": "heat over 9000"
  }}`)

/*
idea from https://stackoverflow.com/questions/28033277/golang-decoding-generic-json-objects-to-one-of-many-formats
verdict: most compact format of the three {dotencoding, mime/multipart, json}, widely available format.
drawback: unsuitable as an abstraction format.
	1) some other new shiny format *will* eventually come along and then the JSON framing format would be limiting.
	2) JSON is difficult to carry anything else inside of itself, especially no binary payloads (-> base64) and most other text-based payloads would also have to be base64-encoded to avoid character conflicts like quotes.
thus an outher envelope abstraction format / header is required, which is to be text-based / string-based.
*/

func main() {
	var m Message
	if err := json.Unmarshal(data, &m); err != nil {
		log.Fatal(err)
	}
	switch m.Type {
	case "event":
		var e Event
		if err := json.Unmarshal([]byte(m.Data), &e); err != nil {
			log.Fatal(err)
		}
		fmt.Println(m.Type, e.Type, e.Creator)
	default:
		log.Fatal("unknown message type")
	}
}
