# The `reporter` container in every authorized hub's stack: this Konstruktor's CLI,
# running `konstruktor hub-report` — see crates/konstruktor-core/src/hubhealth.rs.
#
# Not built from source here. The release already produces static musl binaries for both
# Linux architectures (publish.yaml, `publish-cli`); this only puts the right one in an
# image. The build context is expected to hold them as
#   konstruktor-x86_64-unknown-linux-musl
#   konstruktor-aarch64-unknown-linux-musl
FROM alpine:3

# The coordination server is reached over TLS; rustls needs the roots.
RUN apk add --no-cache ca-certificates

ARG TARGETARCH
COPY konstruktor-*-unknown-linux-musl /tmp/
RUN case "$TARGETARCH" in \
      amd64) triple=x86_64 ;; \
      arm64) triple=aarch64 ;; \
      *) echo "no binary for $TARGETARCH" >&2; exit 1 ;; \
    esac \
 && mv "/tmp/konstruktor-${triple}-unknown-linux-musl" /usr/local/bin/konstruktor \
 && chmod +x /usr/local/bin/konstruktor \
 && rm -f /tmp/konstruktor-*

ENTRYPOINT ["konstruktor"]
CMD ["hub-report"]
