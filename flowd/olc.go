package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net/http"

	"github.com/gorilla/websocket"
)

// TODO wss = ws + TLS security
func startOLC(address string) {
	upgrader := websocket.Upgrader{
		// TODO check for correct WS subprotocol
		ReadBufferSize:    1024,
		WriteBufferSize:   1024,
		EnableCompression: true,
	}
	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		// upgrade to Websocket
		conn, err := upgrader.Upgrade(w, r, nil)
		if err != nil {
			fmt.Println(err)
			return
		}
		fmt.Println("Client subscribed")
		// handle messages
		var fbpMsg JSONMessage
		var respBytes []byte
		for {
			// read Websocket message
			msgType, msg, err := conn.ReadMessage()
			if err != nil {
				connError(conn, "Reading WS message failed: "+err.Error())
				return
			}
			// parse into protocol JSON message
			err = json.Unmarshal(msg, &fbpMsg)
			if err != nil {
				connError(conn, "Parsing JSON inside WS message failed: "+err.Error())
				return
			}
			// parse payload and handle according to sub-protocol and topic/command
			switch fbpMsg.Protocol {
			case "runtime":
				switch fbpMsg.Topic {
				case "getruntime":
					fbpPayload := new(JSONRuntimeGetRuntime)
					err = json.Unmarshal(fbpMsg.Payload, &fbpPayload)
					if err != nil {
						connError(conn, fmt.Sprintf("Unmarshaling payload for %s:%s failed: %s", fbpMsg.Protocol, fbpMsg.Topic, err))
						return
					}
					respBytes, err = handleRuntimeGetRuntime(fbpPayload)
					if err != nil {
						connError(conn, "handleRuntimeGetRuntime:"+err.Error())
						return
					}
				case "packet":
					connError(conn, "Unimplemented")
					return
				case "error": // TODO not clear if this is also valid to receive from client
					connError(conn, "Unimplemented")
					return
				default:
					connError(conn, "Subprotocol 'runtime' got unexpected topic: "+fbpMsg.Topic)
					return
				}
				/* TODO
				case "graph":
				case "component":
				case "network":
				case "trace":
				*/
			default:
				connError(conn, "Unexpected FBP subprotocol: "+fbpMsg.Protocol)
				return
			}
			// send response, if any
			if respBytes != nil {
				err = conn.WriteMessage(msgType, respBytes)
				if err != nil {
					fmt.Println(err)
					return
				}
			}
		}
	})
	/*
		http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
			fmt.Fprint(w, "flowd")
		})
	*/
	/*
		TODO check default parameters for Server:
		ReadTimeout:    10 * time.Second,
		WriteTimeout:   10 * time.Second,
		MaxHeaderBytes: 1 << 20,
	*/
	/* TODO
	if err := http.ListenAndServe(address, nil); err != nil {
		fmt.Println("ERROR starting OLC:", err)
		return
	}
	*/
	log.Fatal(http.ListenAndServe(address, nil))
}

func connError(conn *websocket.Conn, text string) {
	conn.Close()
	fmt.Println(text)
	fmt.Println("Client unsubscribed")
}

func handleRuntimeGetRuntime(payload *JSONRuntimeGetRuntime) ([]byte, error) {
	if !checkSecret(payload.Secret) {
		return nil, errors.New("Unauthenticated")
	}
	respBytes, _ := json.Marshal(JSONRuntimeRuntime{
		//ID: ,	//TODO
		Type:            "flowd",
		ProtocolVersion: "0.5",
		AllCapabilities: []string{},          //TODO
		Capabilities:    []string{},          //TODO
		Name:            "My flowd instance", //TODO
		Graph:           "Unnamed",           //TODO
	})
	return respBytes, nil
}

func checkSecret(secret string) bool {
	// TODO implement
	return true
}

// JSONMessage is the envelope for all manner of requests and responses
type JSONMessage struct {
	Protocol string          `json:"protocol"` // name of the sub-protocol
	Topic    string          `json:"command"`
	Payload  json.RawMessage `json:"payload"` // contains one of the following structs
}

// runtime protocol

// JSONRuntimeError is the generic error for the runtime sub-protocol
type JSONRuntimeError struct {
}

// JSONRuntimeGetRuntime requests general information about the runtime
type JSONRuntimeGetRuntime struct {
	Secret string `json:"secret"`
}

// JSONRuntimePacket contains a packet for a NETIN or from a NETOUT port = exported port
type JSONRuntimePacket struct {
	Port    string `json:"port"`
	Event   string `json:"event"`
	Graph   string `json:"graph"`
	Payload string `json:"payload"`
	Secret  string `json:"secret"`
}

// JSONRuntimePorts is an information packet from the runtime to inform of changed exported ports
type JSONRuntimePorts struct {
	Graph    string            `json:"graph"`
	InPorts  []JSONRuntimePort `json:"inPorts"`
	OutPorts []JSONRuntimePort `json:"outPorts"`
}

// JSONRuntimePort contains information about one port exported by the runtime
type JSONRuntimePort struct {
	Addressable bool   `json:"addressable"`
	ID          string `json:"id"`
	Type        string `json:"type"`
	Required    bool   `json:"required"`
	Description string `json:"description"`
}

// JSONRuntimeRuntime is the response to a getruntime request
type JSONRuntimeRuntime struct {
	ID              string   `json:"id"`              // runtime ID as registered with Flowhub Registry
	Type            string   `json:"type"`            // type of runtime = runtime name
	ProtocolVersion string   `json:"version"`         // supported protocol version; not runtime version
	AllCapabilities []string `json:"allCapabilities"` // complete list of protocol capabilities / features
	Capabilities    []string `json:"capabilities"`    // supported protocol capabilities / features for current user and graph
	Name            string   `json:"label"`           // description or name of this runtime instance
	Graph           string   `json:"graph"`           // currently active graph
}

/*
// graph protocol

type JsonClear struct {
	ID          string
	Name        string
	Library     string
	Main        bool
	Icon        string
	Description string
	Secret      string
}

type JsonAddNode struct {
	ID        string
	Component string
	Metadata  map[string]string
	Graph     string
	Secret    string
}

type JsonRemoveNode struct {
	ID     string
	Graph  string
	Secret string
}

type JsonRenameNode struct {
	From   string
	To     string
	Graph  string
	Secret string
}

type JsonChangeNode struct {
	ID       string
	Metadata map[string]string
	Graph    string
	Secret   string
}

type JsonAddEdge struct {
	Src      *JsonGraphNode
	Tgt      *JsonGraphNode
	Metadata map[string]string
	Graph    string
	Secret   string
}

type JsonGraphNode struct {
	Node  string
	Port  string
	Index int
}

type JsonRemoveEdge struct {
	Graph  string
	Src    *JsonGraphNode
	Tgt    *JsonGraphNode
	Secret string
}

type JsonChangeEdge struct {
	Graph    string
	Metadata map[string]string
	Src      *JsonGraphNode
	Tgt      *JsonGraphNode
	Secret   string
}

type JsonAddInitial struct {
	Graph    string
	Metadata map[string]string
	Src      *JsonInitial
	Tgt      *JsonGraphNode
	Secret   string
}

type JsonInitial struct {
	Data string
}

//TODO

type JsonRemoveInitial struct{}

type JsonAddInport struct{}

type JsonRemoveInport struct{}

type JsonRenameInport struct{}

type JsonAddOutport struct{}

type JsonRemoveOutport struct{}

type JsonRenameOutport struct{}

type JsonAddGroup struct{}

type JsonRemoveGroup struct{}

type JsonRenameGroup struct{}

type JsonChangeGroup struct{}

// component protocol

type JsonList struct{}

type JsonGetSource struct{}

type JsonSource struct{}

type JsonComponent struct{}

// network protocol

type JsonNetworkError struct{}

type JsonStart struct{}

type JsonGetStatus struct{}

type JsonStop struct{}

type JsonPersist struct{}

type JsonDebug struct{}

type JsonEdges struct{}

type JsonStopped struct{}

type JsonStarted struct{}

type JsonStatus struct{}

type JsonOutput struct{}

type JsonProcessError struct{}

type JsonIcon struct{}

type JsonConnect struct{}

type JsonBeginGroup struct{}

type JsonData struct{}

type JsonEndGroup struct{}

type JsonDisconnect struct{}

// trace protocol

type JsonTracingStart struct{}

type JsonTracingStop struct{}

type JsonTracingDump struct{}

type JsonTracingClear struct{}
*/
