# The `scheduler` container of a hub whose jobs a container wakes: this Konstruktor's CLI
# running `konstruktor schedule serve` — see crates/konstruktor-core/src/schedule.rs.
#
# Beside the CLI, the Docker CLI and its compose plugin: a job runs as `compose run` in a
# container of its service's image, through the host's engine (its socket is mounted).
# Not built from source here, like the reporter: the release's static musl binaries are
# put in the image. The build context is expected to hold them as
#   konstruktor-x86_64-unknown-linux-musl
#   konstruktor-aarch64-unknown-linux-musl
FROM alpine:3

RUN apk add --no-cache ca-certificates docker-cli docker-cli-compose tzdata

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
CMD ["schedule", "serve"]
