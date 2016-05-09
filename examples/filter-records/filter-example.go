package main

import (
	"encoding/json"
	"fmt"
	"os"
)

type Message struct {
	Name string
	Body string
	Time int64
}

func main() {
	dec := json.NewDecoder(os.Stdin)
	enc := json.NewEncoder(os.Stdout)
	for {
		var v map[string]interface{}
		if err := dec.Decode(&v); err != nil {
			fmt.Fprintln(os.Stderr, err)
			return
		}
		for k := range v {
			if k != "Name" {
				delete(v, k)
			}
		}
		if err := enc.Encode(&v); err != nil {
			fmt.Fprintln(os.Stderr, err)
			return
		}
	}
}
