#!/usr/bin/env bash

# configuration
FIFOPath="/dev/shm"
GeneratorIN="$FIFOPath/Generator.IN"
CmdIN="$FIFOPath/Cmd.IN"
DisplayIN="$FIFOPath/Display.IN"

# create named pipes (FIFOs)
echo "create named pipes"
mkfifo $GeneratorIN
mkfifo $CmdIN
mkfifo $DisplayIN

function cleanup() {
  killall cron cmd display
  rm -f $GeneratorIN $CmdIN $DisplayIN
}

# handle shutdown
echo "set interrupt handler"
trap "echo ; echo 'clean up' ; cleanup ; exit 0" INT

# IIPs
echo "prepare IIPs"
#TODO how to put the following into a variable while preserving parameter separation?
#GeneratorIIP="-debug -when "*/1 * * * * * *" -to OUT"
#GeneratorIIP=("-debug" "-when" "1 1 1 1 1 1 1" "-to" "OUT")
#echo ${GeneratorIIP[@]}
#exit
#CmdIIP="-debug -mode=each -framing=false /bin/echo 'Hello world!'"

# start components
echo "start components"
bin/cron -outport OUT -outpath $CmdIN -debug -when "*/2 * * * * * *" -to OUT 2>&1 | sed -e 's/^/Generator: /' &
bin/cmd -inport IN -inpath $CmdIN -outport OUT -outpath $DisplayIN -debug -mode=each -framing=false /bin/echo 'Hello world!' 2>&1 | sed -e 's/^/Cmd: /' &
bin/display $DisplayIN 2>&1 | sed -e 's/^/Display: /' &

# wait for network to exit
echo "running..."
read

# clean up
echo "clean up"
cleanup
