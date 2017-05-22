package flowd

// NOTE: explanation at https://github.com/golang/go/issues/359

// BracketOpen generates an opening bracket frame for the given port
func BracketOpen(port string) Frame {
	return Frame{
		Type:     "control",
		BodyType: "BracketOpen",
		Port:     port,
	}
}

// BracketClose generates a closing bracket frame for the given port
func BracketClose(port string) Frame {
	return Frame{
		Type:     "control",
		BodyType: "BracketClose",
		Port:     port,
	}
}

// PortClose generates a port close command/notification for the given port
func PortClose(port string) Frame {
	return Frame{
		Type:     "control",
		BodyType: "PortClose",
		Port:     port,
	}
}
