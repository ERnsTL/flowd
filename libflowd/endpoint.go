package flowd

import (
	"errors"
	"fmt"
	"net"
	"net/url"
	"regexp"
	"strconv"
)

func ParseEndpointURL(value string) (url *url.URL, err error) {
	url, err = url.Parse(value)
	if err != nil {
		return nil, errors.New("could not parse flag value as URL: " + err.Error())
	}
	// convert just-parsed URL to endpoint and replace *this* endpoint
	//*e = *(*endpoint)(e2)
	// filter unallowed URL parts; only scheme://host:port is allowed
	// NOTE: url.Opaque is eg. localhost:0 -> usually present, but can be left out to mean 0.0.0.0
	/*
		if url.Opaque == "" {
			return nil, errors.New("opaque part = host+port nil")
		}
	*/
	if url.User != nil {
		if url.User.String() == "" && (url.Scheme == "unix" || url.Scheme == "unixpacket") {
			// OK, this allows for transferring the "@" via the URL for Linux's abstract Unix domain sockets
		} else {
			return nil, errors.New("user part not nil: " + url.User.String())
		}
	}
	if url.Path != "" {
		if url.Scheme == "unix" || url.Scheme == "unixpacket" {
			// OK, this allows for transferring a path-bound Unix domain address
		} else {
			return nil, errors.New("path part not nil (" + url.Path + ")")
		}
	}
	if url.RawPath != "" {
		return nil, errors.New("raw path part not nil")
	}
	if url.RawQuery != "" {
		return nil, errors.New("raw query part not nil")
	}
	if url.Fragment == "" {
		return nil, errors.New("fragment nil, must be name of port")
	} else {
		// check for well-formed port name
		// NOTE: \w is word characters, (?: is grouping without extraction
		re := regexp.MustCompile(`(\w+)(?:\[(\d+)\])?`)
		// NOTE: [0] is match of entire exp, [1] is port name, [2] is array port index
		matches := re.FindStringSubmatch(url.Fragment)
		if matches[0] != url.Fragment {
			return nil, errors.New("port name malformed, must be \\w+ or \\w+[index]")
		} else if len(matches[1]) > 1000 {
			return nil, errors.New("port name too long, maximum allowable is 1000 UTF-8 runes")
		} else if len(matches[2]) > 4 {
			return nil, errors.New("port name array index too long, maximum allowable is 4 digits")
		}
		// TODO save the port name and index somewhere inside of ourselves to avoid duplicate work
	}
	// check for required URL parts
	if url.Scheme == "" {
		return nil, errors.New("scheme missing")
	}
	if matched, err := regexp.MatchString(`^(tcp|tcp4|tcp6|unix|unixpacket)$`, url.Scheme); err != nil {
		return nil, fmt.Errorf("could not parse scheme: %s", err)
	} else if !matched {
		return nil, errors.New("unimplemented scheme: only {tcp,tcp4,tcp6,unix,unixpacket} allowed")
	}
	//TODO optimize unix- and unixpacket-specific logic all over this function
	if url.Host == "" {
		if url.Scheme == "unix" || url.Scheme == "unixpacket" {
			if url.User != nil {
				// OK, this allows for transferring the "@" via the URL for Linux's abstract Unix domain sockets
				url.Host = "@" + url.Host
			} else {
				// OK, this allows for transferring a path-bound Unix domain address
			}
		} else {
			return nil, errors.New("missing host:port or //")
		}
	} else {
		if url.Scheme == "unix" || url.Scheme == "unixpacket" {
			if url.User != nil {
				// OK, this allows for transferring the "@" via the URL for Linux's abstract Unix domain sockets
				url.Host = "@" + url.Host
			}
		} else {
			// TCP, UDP have [host]:[port] format
			_, portStr, err := net.SplitHostPort(url.Host)
			if err != nil {
				return nil, errors.New("host and/or port unvalid: " + err.Error())
			}
			var port int
			if portStr == "" {
				port = 0
			} else {
				port, err = strconv.Atoi(portStr)
				if err != nil {
					return nil, errors.New("port malformed, only numbers allowed: " + err.Error())
				}
				if port < 0 || port > 65535 {
					return nil, errors.New("port out of range: allowed range is [0;65535]")
				}
			}
			// TODO save the int port and addr somewhere inside ourselves to avoid duplicate work
		}
	}
	return url, nil
}
