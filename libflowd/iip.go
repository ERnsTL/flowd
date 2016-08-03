package flowd

import (
	"bufio"
	"fmt"
	"os"
)

// get configuration from IIP = initial information packet/frame
//TODO what if several IIPs are expected, one each on multiple ports?
func GetIIP(port string) (string, error) {
	stdin := bufio.NewReader(os.Stdin) //TODO might this suck STDIN dry = read more than what is required for 1 frame, meaning lose any possibly following frames?
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
