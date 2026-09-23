# The reporter image, built from this checkout rather than from a release.
#
# For development: to run a reporter with changes that are not released yet. Tagged as
# the image generation writes, compose finds it locally and never asks the registry:
#
#   docker build -f docker/reporter-source.Dockerfile -t jhnnsrs/reporter:latest .
#
# Releases use reporter.Dockerfile, which packages the binaries CI already built.
FROM rust:1-alpine AS build
# musl-gcc and friends, for the C and assembly in `ring`.
RUN apk add --no-cache build-base
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p konstruktor-cli

FROM alpine:3
RUN apk add --no-cache ca-certificates
COPY --from=build /src/target/release/konstruktor /usr/local/bin/konstruktor
ENTRYPOINT ["konstruktor"]
CMD ["hub-report"]
