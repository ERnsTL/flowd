#!/usr/bin/env bash

# configuration
FIFOPath="/dev/shm"
ChatIN="$FIFOPath/Chat.IN"
TCPIN="$FIFOPath/TCP.IN"

# create named pipes (FIFOs)
echo "create named pipes"
mkfifo $ChatIN
mkfifo $TCPIN

function cleanup() {
  killall chat ; killall tcp-server
  rm $ChatIN ; rm $TCPIN
}

# handle shutdown
echo "set interrupt handler"
trap "echo ; echo 'clean up' ; cleanup ; exit 0" INT

# IIPs
echo "prepare IIPs"
TCPIIP="tcp4://localhost:4000"

# start components
echo "start components"
bin/tcp-server -inport IN -inpath $TCPIN -outport OUT -outpath $ChatIN $TCPIIP 2>&1 | sed -e 's/^/TCP: /' &
bin/chat -inport IN -inpath $ChatIN -outport OUT -outpath $TCPIN 2>&1 | sed -e 's/^/Chat: /' &

# wait for network to exit
echo "running..."
read

# clean up
echo "clean up"
cleanup
