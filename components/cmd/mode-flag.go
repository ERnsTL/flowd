package main

import "fmt"

// OperatingMode is a type redifinition for the enumeration below.
type OperatingMode int

// One and Each are the two operating modes.
// One = One running instance, Each starts one instance per IP.
const (
	One OperatingMode = iota
	Each
)

// Implement flag.Value interface
func (op *OperatingMode) String() string {
	op2 := *op //FIXME how to do this correctly? type switch?
	switch op2 {
	case One:
		return "one call handling all input IPs"
	case Each:
		return "one call for each input IP"
	default:
		if op == nil {
			return "nil"
		}
		return "ERROR value out of range"
	}
}

// Set the operating mode. Also required for the flag.Value interface.
func (op *OperatingMode) Set(value string) error {
	switch value {
	case "one":
		*op = One
	case "each":
		*op = Each
	default:
		return fmt.Errorf("set of allowable values for operating mode is {one, each}")
	}
	return nil
}
