FROM ubuntu:bionic-20210416 as deps

RUN apt-get -y update && apt-get -y install \
    clang \
    curl \
    libssl-dev \
    pkg-config

WORKDIR /build
COPY rust-toolchain .
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal -c clippy rustfmt --default-toolchain $(cat rust-toolchain)
ENV PATH=$PATH:/root/.cargo/bin
RUN cargo install cargo-fuzz

COPY rust-nightly-toolchain .
RUN rustup toolchain install $(cat rust-nightly-toolchain)

FROM deps as src

COPY Cargo.toml Cargo.lock ./
COPY assert-allocations/Cargo.toml assert-allocations/
COPY fuzz/Cargo.toml fuzz/
COPY hyper-websocket-lite/Cargo.toml hyper-websocket-lite/
COPY websocket-codec/Cargo.toml websocket-codec/
COPY websocket-lite/Cargo.toml websocket-lite/

RUN mkdir -p \
    assert-allocations/src \
    hyper-websocket-lite/examples \
    hyper-websocket-lite/src \
    websocket-codec/benches \
    websocket-codec/examples \
    websocket-codec/src \
    websocket-lite/examples \
    websocket-lite/src

RUN touch \
    assert-allocations/src/lib.rs \
    hyper-websocket-lite/src/lib.rs \
    websocket-codec/src/lib.rs \
    websocket-lite/src/lib.rs

RUN \
    echo "fn main() {}" > hyper-websocket-lite/examples/autobahn-server.rs && \
    echo "fn main() {}" > hyper-websocket-lite/examples/hello-world-server.rs && \
    echo "fn main() {}" > websocket-codec/benches/bench.rs && \
    echo "fn main() {}" > websocket-codec/examples/wsinspect.rs && \
    echo "fn main() {}" > websocket-lite/examples/async-autobahn-client.rs && \
    echo "fn main() {}" > websocket-lite/examples/autobahn-client.rs && \
    echo "fn main() {}" > websocket-lite/examples/hello-world-client.rs && \
    echo "fn main() {}" > websocket-lite/examples/wsdump.rs

ENV RUSTFLAGS=-Dwarnings
RUN cargo build --release --workspace --exclude fuzz --all-targets

COPY . .
RUN find . -name "*.rs" | grep -v "^\./target" | xargs touch

FROM src as build
RUN cargo build --release --workspace --exclude fuzz --all-targets

FROM build as fuzz
RUN mv rust-nightly-toolchain rust-toolchain
RUN cargo fuzz build

FROM ubuntu:bionic-20210416 as app

RUN apt-get -y update && apt-get -y install \
    ca-certificates \
    netcat \
    openssl \
    python-pip \
    python2.7 \
    python3-pip

RUN pip2 install \
    autobahntestsuite

RUN pip3 install \
    websockets

WORKDIR /app

COPY --from=build \
    /build/target/release/examples/async-autobahn-client \
    /build/target/release/examples/autobahn-client \
    /build/target/release/examples/autobahn-server \
    /build/target/release/examples/hello-world-client \
    /build/target/release/examples/hello-world-server \
    /build/target/release/examples/wsdump \
    ./
