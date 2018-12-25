package noflo

import (
	"encoding/json"
	"io/ioutil"
	"os"
)

/*
  NOTE: using the JSON format and schema as of 2018-12-25 at:
  https://noflojs.org/documentation/graphs/#json
  https://github.com/flowbased/fbp/blob/master/schema/graph.json
  https://github.com/flowbased/fbp/blob/master/schemata/graph.yaml
*/

// Graph is the root element
type Graph struct {
	CaseSensitive bool              `json:"caseSensitive"`
	Properties    GraphProperties   `json:"properties"`
	Inports       map[string]NWPort `json:"inports"`
	Outports      map[string]NWPort `json:"outports"`
	//Groups        []ProcessGroup  `json:"groups"`
	Processes   map[string]Process `json:"processes"`
	Connections []Connection       `json:"connections"`
}

type GraphProperties struct {
	Name        string                  `json:"name"`
	Environment GraphRuntimeEnvironment `json:"environment"`
	Description string                  `json:"description"`
	Icon        string                  `json:"icon"`
}

type GraphRuntimeEnvironment struct {
	Type    string `json:"type"`
	Content string `json:"content"`
}

// NWPort definies an exported network/graph input or output port
type NWPort struct {
	Process string `json:"process"`
	Port    string `json:"port"`
	//Metadata Metadata `json:"metadata"`
}

/*
type Metadata struct {
	X int `json:"x"`
	Y int `json:"y"`
}
*/

/*
type ProcessGroup struct {
	Name     string            `json:"name"`
	Nodes    []string          `json:"nodes"`
	Metadata map[string]string `json:"metadata"` // can contain: description
}
*/

// A Process is an instance of a component
type Process struct {
	Component string `json:"component"`
	//Metadata  Metadata `json:"metadata"`
}

// A Connection is a run-time connection between processes
type Connection struct {
	//TODO
	Source ConnectionEndpoint `json:"src"`
	Target ConnectionEndpoint `json:"tgt"`
	Data   string             `json:"data"` // presumably for IIPs?
	//Metadata ConnectionMetadata `json:"metadata"`
}

type ConnectionEndpoint struct {
	Process string `json:"process"`
	Port    string `json:"port"`
	Index   int    `json:"index"`
}

/*
type ConnectionMetadata struct {
	Route  int      `json:"route"`
	Schema *url.URL `json:"schema"` // URL of JSON schema for validating IPs passing through this connection/edge
	Secure bool     `json:"secure"`
}
*/

// ParseNetwork parses a .json NoFlo graph JSON file into a Graph data structure, defined above
func ParseNetwork(filePath string) (net *Graph, err error) {
	// open .json Graph JSON file
	graphFile, err := os.Open(filePath)
	if err != nil {
		return nil, err
	}
	defer graphFile.Close()

	// read the file contents into a byte array
	graphBytes, err := ioutil.ReadAll(graphFile)
	if err != nil {
		return nil, err
	}

	// parse the JSON data into the Graph data structure
	root := Graph{}
	err = json.Unmarshal(graphBytes, &root)
	if err != nil {
		return nil, err
	}

	return &root, nil
}
