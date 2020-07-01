FROM ubuntu:bionic-20200526 as deps

RUN apt-get -y update

RUN apt-get -y install \
    clang \
    curl \
    libssl-dev \
    pkg-config

WORKDIR /build
COPY rust-toolchain .
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain $(cat rust-toolchain)
ENV PATH=$PATH:/root/.cargo/bin
RUN rustup component add clippy
RUN cargo install cargo-fuzz

COPY rust-nightly-toolchain .
RUN rustup toolchain install $(cat rust-toolchain)

FROM deps as src

COPY Cargo.toml Cargo.lock ./
COPY assert-allocations/Cargo.toml assert-allocations/
COPY fuzz/Cargo.toml fuzz/
COPY hyper-websocket-lite/Cargo.toml hyper-websocket-lite/
COPY websocket-codec/Cargo.toml websocket-codec/
COPY websocket-lite/Cargo.toml websocket-lite/
RUN cargo fetch

COPY . .

FROM src as build

ENV RUSTFLAGS=-Dwarnings
RUN cargo test --release
RUN cargo build --release --workspace --exclude fuzz
RUN cargo clippy --release

FROM src as fuzz

RUN mv rust-nightly-toolchain rust-toolchain
RUN cargo fuzz build

FROM ubuntu:bionic-20200526 as app

RUN apt-get -y update

RUN apt-get -y install \
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
