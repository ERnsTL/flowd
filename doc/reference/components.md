## Included Components

* Repeat
* Drop
* Output
* LibComponent (work in progress - for loading components from C API libraries)
* Unix domain socket server
  * (planned) support for abstract address
* File reader
* File tailing resp. following including detection of replaced or truncated file
* File writer
* Trim for removing trailing and heading whitespace
* Split lines
* Count for packets, size of packets and sum of packet values
* Cron for time-based events, with extended 7-parameter form and granularity up to seconds
* Cmd for calling sub-processes or "shelling out", enabling re-use of any existing programs and their output or for transformation of data, sending data out to the sub-process etc. The sub-process commandline with arguments can be configured. TODO add more info about the features of this component
* Hasher for calculating hash value of packets, supports xxHash64
* Equals routing based on matching IP content
* simple HTTP client
* simple HTTP server, supporting multiple HTTP routes
* Muxer to merge multiple connections into one output for components not able to handle array inports
* MQTT publisher
* MQTT subscriber
* Redis Pub/Sub publisher
* Redis Pub/Sub subscriber
* IMAP e-mail append (sender)
* IMAP e-mail fetch+idle (receiver)
* OpenAI Chat component ("ChatGPT component")
* Tera template component, which includes control flow and logic usable for simple scripts
* Regexp data extraction component
* Text replacement
* Compression and decompression in XZ (LZMA2) format
* Compression and decompression in Brotli format
* Unix domain socket client (path-based and abstract socket addresses, support for SEQPACKET)
* strip HTML tags to get the contained content for further processing
* WebSocket client
  * (planned) retry on connection establishment
  * (planned) reconnection
* TCP client
* TLS client
* TCP server
* TLS server
* WebSocket server
* Zeroconf service publishing and browsing based on mDNS (multicast DNS) resp. Bonjour and DNS-SD
* JSON query component using jaq/jq filter syntax
* XPath filtering on HTML and simple XML data (note that CSS selectors are a subset of XPath query and can be converted into it)
* SSH client (without using OpenSSH client or libssh)
  * (planned) streaming capability of remote program output
* Telegram component using Bot API supporting text messages
* Matrix client component
