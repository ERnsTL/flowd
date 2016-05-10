package flowd_test

import (
	"bytes"
	"testing"

	"github.com/ERnsTL/flowd/libflowd"
)

/*
func TestConfigMalformed(t *testing.T) {
	var err error
	for i, config := range nokconfigs {
		_, err = GetAllNews(config)
		assert.Error(t, err, "config malformed test entry #%d", i)
	}
}
*/

func TestHasFunctionParseFrame(t *testing.T) {
	flowd.ParseFrame(nil)
}

func TestHasStructFrame(t *testing.T) {
	var _ flowd.Frame
}

func TestFrameHasRequiredMethods(t *testing.T) {
	type IWantThisMethod interface {
		Marshal(*bytes.Buffer) error
	}
	var _ IWantThisMethod = &flowd.Frame{}
}
