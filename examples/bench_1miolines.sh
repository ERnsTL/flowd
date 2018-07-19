#!/bin/bash
if [ $# -ne 1 ]
then
	echo "usage: $0 <start|stop|clean>"
	exit 128
fi

# configuration
echo "configuration"
FIFOPath="/dev/shm"
ReaderIN="$FIFOPath/Reader.IN"
LineSplitterIN="$FIFOPath/LineSplitter.IN"
CounterIN="$FIFOPath/Counter.IN"
DisplayIN="$FIFOPath/Display.IN"

function cleanup() {
	echo "clean up"
	rm -f $ReaderIN
	rm -f $LineSplitterIN
	rm -f $CounterIN
	rm -f $DisplayIN

	#TODO improve - kill by PID
	killall --quiet file-read
	killall --quiet split-lines
	killall --quiet counter
	killall --quiet display
}

# clean up
cleanup

# set up FIFOs
echo "create FIFOs"
mkfifo $ReaderIN
mkfifo $LineSplitterIN
mkfifo $CounterIN
mkfifo $DisplayIN

# send IIPs
echo "send IIPs"
ReaderIIP="testdata-1M-lines"
CounterIIP="-packets -quiet"

echo "start components"

# read file
# https://unix.stackexchange.com/questions/251388/prefix-and-suffix-strings-to-each-output-line-from-command#251414
bin/file-read -outport OUT -outpath $LineSplitterIN $ReaderIIP 2>&1 | sed -e 's/^/Reader: /' &

# split lines
bin/split-lines -inport IN -inpath $LineSplitterIN -outport OUT -outpath $CounterIN 2>&1 | sed -e 's/^/LineSplitter: /' &

# count packets
#TODO (on input close, display the count, then exit)
bin/counter -inport IN -inpath $CounterIN -outport OUT -outpath $DisplayIN $CounterIIP 2>&1 | sed -e 's/^/Counter: /' &

# display (the packet count)
bin/display $DisplayIN 2>&1 | sed -e 's/^/Display: /'

# clean up
cleanup
