## History and Reference of Rationale


### Change of framing format

This was done in commit 814ce0716cb7aefd817213e7840e55051e8541b5 of 2017-06-12.

See issue #61. In summary, most programming languages resp. their standard libraries or external libraries do not expose bare HTTP or MIME header parsing as public methods or re-usable functions. Therefore, being compatible to that saves nobody effort, because the parsing functionality is not exposed separately.

A different format is thus needed.

Requirements:

* MIME-related or otherwise key-value based,
* binary-capable,
* text-based and
* simple to parse,
* re-usable = existing framing format.

The choice thus fell to the simple STOMP v1.2 framing format. It is basically an optimized, simplified MIME format. And of that, only a subset is currently used and implemented and a few minor modifications apply. So, if a component developer has to implement a parser and marshaler, then it will be very little effort to implement.

Benchmarks made for issue #92 show 40 to 50 % lower runtime for ```Marshal()``` and 15 % lower runtime for ```ParseFrame()```. On the wire, the format saves 25 % bytes for an average case.

Also, a version marker byte was introduced, in case the format has to be changed again in the future, though it is unlikely, because a format fulfilling the requirements cannot be much more simple.

In total, ```flowd``` stays close to the goal of not inventing new formats and protocols and making it simple for component writers to serialize and deserialize IPs.


### Removal of ```launch```

The functionality of the ```launch``` program was moved into ```flowd```. This was done in commit 5a50e12a687818e4551b7cd380ea6cef63560a21 of 2017-03-26.

The reason for the change in architecture was the desire to incorporate online network reconfiguration (OLC).

The ```launch``` program was really just an indirection and wrapper for communicating with the components, like a shepherd for that component. But once started, it would have been unproportionally difficult to propagate changes in connections to ```launch```. So instead of coming up with a protocol to communicate endpoint change requests to ```launch```, the decision was to incorporate its core into ```flowd```, where network reconfiguration can be achieved much more easily. This *concern* is thus concentrated inside ```flowd```.

Before the change in architecture:

* ```launch``` took care of network endpoints.
* IPs were sent through UNIX domain sockets, UDP or TCP. Just for a local transfer between processes.
* Adding more network transports would mean that these would have to be incorporated into ```launch```. Each transport behaves slightly different and has different options, which would be difficult to support through one interface - or endpoint URLs would grow to become incomprehensible.
* ```flowd``` would have to be involved also with the endpoint URLs and had to have some understanding of each transport to know how to associate each end of a connection which each other.
* Having many endpoint types inside ```launch``` is not scalable and is not really the *concern* of ```launch``` or the FBP runtime in general.
* Would have lead to too many endpoint types = network transports inside ```launch``` and that was not scalable.
* ```launch``` had unnecessary complexity in configuration and program parameters.
* Difficult-to-read endpoint URLs.
* Quite some code in ```flowd``` was needed to even generate the parameters for ```launch```.
* More copying than necessary across process boundaries, e.g. from component to ```launch```, over a UNIX socket endpoint to the other ```launch``` and into the component.

After the change:

* Components are now launched and monitored directly by ```flowd```.
* Network transports can easily be added by a set of components.
* Network bridges can also simply be constructed using components.
* Drawback: ```flowd``` does not know that this component is one end of a network bridge.
* The responsibility and code for building and maintaining a network will allow for easier network reconfiguration in the future.
* Less copying as delivery of IPs between components is now done inside ```flowd``` using Go channels.
* Much higher performance.


### UnixFBP concept and change of transport medium

The transport medium was changed from STDIN/STDOUT through ```flowd``` to direct communication between the components via named pipes, which are managed by the operating system kernel. This was done in the commits d26901554b737c9d9ede94e02206a0a8c3922de2 and b5a4991e994506d9c9e2c3e2fd1fae47cd04cbac of 2018-07-19 with prototype work going back to 2018-07-10.

* IPC type resp. transport medium:
  * named pipes resp. FIFOs
  * should also be available on Windows
  * components communicate directly without passing through a daemon
  * only one serialization and one deserialization needed for a graph edge transition instead of two and two
* graph definition language:
  * shell script -- has problems:
  * these become large even for simple applications
  * every program is started in a subshell -- huge waste
  * much of the surrounding functionality is duplicated for each application shell script
  * need a generator some sort -- or a *flowd* again, which only starts and stops the network
  * a concise declarative language to define networks (.fbp data format) would be useful
  * **Result**: adapt *flowd* to named pipes and convert IIPs into ARGV port to component arguments
  * also simplifies ```flowd```
  * and still possible to run via a shell script for testing if need be
* start and stop method:
  * using PIDs; KILL -> components can shut down in order as defined by start and stop action
* resume (checkpointing):
  * just keep the named pipes as they are and start again
  * drain all named pipes and save to files, then fill them again on resume
* monitoring, tracing:
  * using regular unix tools (strace, stat on named pipe files and file descriptors, lsof)
  * tracing using custom format
  * syscallx.Tee() which acts like tee = copies pipe contents without draining it -> pull copy
* component metadata:
  * file system extended attributes on the named pipe files
  * list of components: ls bin/
  * list of connections: ls fifos/
* network testing:
  * insert data: echo testdata > fifo
* distributed operation, extending the processing network over LAN/WAN:
  * share the FIFO, which is a file, over a network file system like NFS, sshfs, 9P etc.
  * use a "network pipe" like ssh, netcat, socat etc.
  * [several solutions](https://unix.stackexchange.com/questions/151056/how-can-i-pipe-data-over-network-or-serial-to-the-display-of-another-linux-machi#151058)
  * more options for this when using FIFO than unix socket

Design decisions (can be revised if other solutions prove to be better):

* Packet transport medium:
  * named pipe aka FIFO
    * file-based
    * file operations are implemented everywhere and in all programming languages
    * optimization possibility using splice system call (Linux) = direct fd-to-fd copy in kernel space
    * multi-writer support is limited by atomic message size, see POSIX PIPE_BUF; 4096 in Linux; 512 in other implementations; this might be a limitation of FIFOs as the transport medium; OTOH array ports should solve this issue
    * multi-reader support would be possible if messages were of the same size
    * can start writer before reader is there -- open(3) will block until other end is there; FIFO is a connection no matter what even with O_NONBLOCK, not a mailbox
    * faster than unix sockets for message sizes smallen than 64K ([source](https://github.com/avsm/ipc-bench))
    * connection handling logic is much simpler than any type of socket
  * Unix sockets
    * also widely implemented and available in most programming languages
    * server needs to be up before client can connect (perfect ordering or retries required -> cannot start network in parallel)
    * bidirectional, but not required in FBP (except for ACKs maybe) -- would be best choice for actor system
    * message / datagram semantics -- SEQPACKET mode would be best = connection-oriented (can detect other side disconnecting) and message-oriented
  * shared memory / memory mapping / mmap:
    * would be fastest, but not possible due to integration of different programming languages (which align their data differently, have different data types, have their own GC etc.)
    * chunking size / granularity can be decided based on performance requirements by developer anyway
    * the exact way of how to "do shared memory" needs to be agreed upon for all programs; it's complex and needs to be done carefully
    * not possible in all programming languages
  * POSIX message queues
    * not file-based nor socket-based
    * not widely implemented in programming languages
    * Golang: syscallx.Syscall() and call msgsend() and msgrecv())
    * message / datagram semantics (reduce need for framing format)
* One-inport style or dedicated FIFOs per in-/outport?
  * one-FIFO style: can easily add a port since all packets are coming in over this anyway
  * dedicated: FIFO close = port close -> no need for a PortClosed notification via IP
  * one-FIFO style: easier to hit the multiple-writer limitation
  * Result: use a dedicated pipe per port
* Use IIPs for parametrization or Unix argv program parameters?
  * IIPs = within the FBP framework; can be generated from another component
  * OTOH, the program parameters can also be read from a FIFO, the contents of which could be generated a program resp. component
  * and reading from an IIP via a CONF inport or a file can be added on anyway
  * Result: either way is generally fine, but with the exception of online/runtime reconfiguration, parametrization is mostly done only once and the most efficient way is program arguments

After the change:

* Networks use 53 percent the real time = 1.87 as fast.
* Networks use 35 percent the CPU time (user+sys) = 2.65 times as fast.
