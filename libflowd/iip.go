package flowd

import (
	"bufio"
	"fmt"
)

// get configuration from IIP = initial information packet/frame
// NOTE: reads a single/next IIP -> if multiple are expected on different ports, call multiple times
// NOTE: reason for handing over reference to existing STDIN reader is because local readers suck the buffer dry -> if there are multiple frames waiting, these following would be discarded
func GetIIP(port string, stdin *bufio.Reader) (string, error) {
	if iip, err := ParseFrame(stdin); err != nil {
		return "", fmt.Errorf("ERROR getting IIP from STDIN: %s", err)
	} else {
		if iip.BodyType != "IIP" {
			return "", fmt.Errorf("ERROR: data type of IIP is not 'IIP' but '%s' - Exiting.", iip.BodyType)
		} else if iip.Port != port {
			return "", fmt.Errorf("ERROR: port of IIP is not '%s' but '%s' - Exiting.", port, iip.Port)
		}
		return (string)(iip.Body), nil
	}
}
