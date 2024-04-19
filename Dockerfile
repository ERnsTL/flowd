# start from the Rust base image
# stable
#FROM rust:1.77-bookworm AS build
# nightly
FROM rustlang/rust:nightly-bookworm AS build

# copy files
WORKDIR /src
# TODO optimize - this directory can be big if you are also building outside of the container
COPY . /src/

# install cmake for aws-lc-sys for rustls
# APT Update
RUN apt-get --allow-releaseinfo-change update
RUN apt-get update && apt-get upgrade -y
# APT install (base) packages
RUN apt-get install -y build-essential cmake libboost-all-dev pkg-config

# compile
# debug
RUN set -xe; cargo build
# release
#RUN set -xe; cargo build --release

# copy result files
FROM scratch
COPY --from=build /src/target/release/flowd-rs /flowd-rs
COPY --from=build /lib/x86_64-linux-gnu/liblzma.so.5 /lib/x86_64-linux-gnu/
COPY --from=build /lib/x86_64-linux-gnu/libssl.so.3 /lib/x86_64-linux-gnu/
COPY --from=build /lib/x86_64-linux-gnu/libcrypto.so.3 /lib/x86_64-linux-gnu/
COPY --from=build /lib/x86_64-linux-gnu/libm.so.6 /lib/x86_64-linux-gnu/
COPY --from=build /lib/x86_64-linux-gnu/libc.so.6 /lib/x86_64-linux-gnu/
COPY --from=build /lib/x86_64-linux-gnu/libgcc_s.so.1 /lib/x86_64-linux-gnu/
COPY --from=build /lib64/ld-linux-x86-64.so.2 /lib64/
