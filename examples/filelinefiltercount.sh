#!/usr/bin/env bash

# configuration
FIFOPath="/dev/shm"
ReaderIN="$FIFOPath/Reader.IN"
LineSplitterIN="$FIFOPath/LineSplitter.IN"
FilterIN="$FIFOPath/Filter.IN"
CounterIN="$FIFOPath/Counter.IN"
DisplayIN="$FIFOPath/Display.IN"

# create named pipes (FIFOs)
echo "create named pipes"
mkfifo $ReaderIN
mkfifo $LineSplitterIN
mkfifo $FilterIN
mkfifo $CounterIN
mkfifo $DisplayIN

function cleanup() {
  killall --quiet file-read split-lines packet-filter-string counter display
  rm -f $ReaderIN $LineSplitterIN $FilterIN $CounterIN $DisplayIN
}

# handle shutdown
echo "set interrupt handler"
trap "echo ; echo 'clean up' ; cleanup ; exit 0" INT

# IIPs
echo "prepare IIPs"
# comment in/out as needed
ReaderIIP="/var/log/syslog"
FilterIIP="-pass -or cron network sudo"
#CounterIIP="-packets"
#CounterIIP="-size"

# start components
echo "start components"
# comment in/out as needed and adapt connections -> can count lines, bytes (file size), filter lines, count number of filtered lines etc.
#bin/file-read -outport OUT -outpath $LineSplitterIN $ReaderIIP 2>&1 | sed -e 's/^/Reader: /' &
#bin/split-lines -inport IN -inpath $LineSplitterIN -outport OUT -outpath $FilterIN $LineSplitterIIP 2>&1 | sed -e 's/^/LineSplitter: /' &
#bin/packet-filter-string -inport IN -inpath $FilterIN -outport OUT -outpath $CounterIN $FilterIIP 2>&1 | sed -e 's/^/Filter: /' &
#bin/counter -inport IN -inpath $CounterIN -outport OUT -outpath $DisplayIN $CounterIIP 2>&1 | sed -e 's/^/Counter: /' &
#bin/display $DisplayIN 2>&1 | sed -e 's/^/Display: /' &

#bin/file-read -outport OUT -outpath $LineSplitterIN $ReaderIIP 2>&1 | sed -e 's/^/Reader: /' &
#bin/split-lines -inport IN -inpath $LineSplitterIN -outport OUT -outpath $FilterIN $LineSplitterIIP 2>&1 | sed -e 's/^/LineSplitter: /' &
#bin/packet-filter-string -inport IN -inpath $FilterIN -outport OUT -outpath $DisplayIN $FilterIIP 2>&1 | sed -e 's/^/Filter: /' &
#bin/display $DisplayIN 2>&1 | sed -e 's/^/Display: /'

exec bin/file-read -outport OUT -outpath $LineSplitterIN -quiet $ReaderIIP &
exec bin/split-lines -inport IN -inpath $LineSplitterIN -outport OUT -outpath $FilterIN $LineSplitterIIP -quiet &
exec bin/packet-filter-string -inport IN -inpath $FilterIN -outport OUT -outpath $DisplayIN $FilterIIP -quiet &
bin/display $DisplayIN

# wait for network to exit
#echo "running..."
#read

# clean up
#echo "clean up"
cleanup
