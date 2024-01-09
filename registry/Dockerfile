FROM rust:1.75.0-bookworm as builder

## DOWNLOAD PROTO COMPILER
##
ARG PB_VERSION=25.1
ARG PB_REL="https://github.com/protocolbuffers/protobuf/releases"


WORKDIR /tmp/protobuf
RUN wget ${PB_REL}/download/v${PB_VERSION}/protoc-${PB_VERSION}-linux-x86_64.zip && \
    unzip protoc-${PB_VERSION}-linux-x86_64.zip -d ./proto

RUN chmod 755 -R ./proto/bin
ENV BASE=/usr
RUN cp ./proto/bin/protoc ${BASE}/bin/
RUN cp -R ./proto/include/* ${BASE}/include/

WORKDIR /usr/src/buffrs
RUN cd /usr/src/buffrs
COPY . .
RUN cd /usr/src/buffrs/registry && cargo build && cargo install --path .

FROM debian:bookworm-slim

COPY --from=builder /usr/local/cargo/bin/buffrs-registry /usr/local/bin/registry

ENV LISTEN="0.0.0.0:4367"

EXPOSE 4367/tcp

CMD ["registry"]
