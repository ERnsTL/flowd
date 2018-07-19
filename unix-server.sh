#!/usr/bin/env bash

# UNIX chat/console server
# connect using, for example:
#   socat - UNIX-CLIENT:/tmp/myfbpchatserver
#   socat - ABSTRACT-CLIENT:myfbpchatserver/test

# configuration
FIFOPath="/dev/shm"
ChatIN="$FIFOPath/Chat.IN"
UnixIN="$FIFOPath/Unix.IN"

# create named pipes (FIFOs)
echo "create named pipes"
mkfifo $ChatIN
mkfifo $UnixIN

function cleanup() {
  killall chat ; killall unix-server
  rm $ChatIN ; rm $UnixIN
}

# handle shutdown
echo "set interrupt handler"
trap "echo ; echo 'clean up' ; cleanup ; exit 0" INT

# IIPs
echo "prepare IIPs"
# abstract UNIX socket are also supported, e.g. @path/path/path
UnixIIP="unix:@myfbpchatserver/test"
# regular filesystem-bound UNIX socket:
#UnixIIP="unix:/tmp/myfbpchatserver"
# packet-oriented UNIX sockets (SEQPACKET) are also supported (untested, TODO):
#UnixIIP="unixpacket:@myfbpchatserver/test"
#UnixIIP="unixpacket:/tmp/myfbpchatserver"

# start components
echo "start components"
bin/unix-server -inport IN -inpath $UnixIN -outport OUT -outpath $ChatIN $UnixIIP 2>&1 | sed -e 's/^/Unix: /' &
bin/chat -inport IN -inpath $ChatIN -outport OUT -outpath $UnixIN 2>&1 | sed -e 's/^/Chat: /' &

# wait for network to exit
echo "running..."
read

# clean up
echo "clean up"
cleanup
