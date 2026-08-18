# Build and package Mousr for the platform selected by Docker Buildx.

FROM rust:1-bookworm AS build

ARG VERSION
ARG GIT_SHA=unknown
ENV MOUSR_GIT_SHA="${GIT_SHA}"
WORKDIR /build

RUN apt-get update \
    && apt-get install --no-install-recommends -y \
       libwayland-dev \
       libxkbcommon-dev \
       pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY assets ./assets
COPY VERSION ./VERSION

RUN cargo build --release --locked \
    && install -Dm755 target/release/mousr /build/mousr


FROM debian:bookworm AS package

ARG VERSION
ARG TARGETARCH
ARG INCLUDE_APPIMAGE=0

ENV VERSION="${VERSION}" \
    TARGETARCH="${TARGETARCH}" \
    INCLUDE_APPIMAGE="${INCLUDE_APPIMAGE}" \
    PACKAGE_ROOT=/build \
    OUTPUT_DIR=/output

RUN apt-get update \
    && apt-get install --no-install-recommends -y \
       dpkg-dev \
       python3 \
       rpm \
       tar \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY --from=build /build/mousr ./mousr
COPY VERSION release.toml ./
COPY yesb/package.py ./yesb/package.py

RUN python3 yesb/package.py


FROM scratch

COPY --from=package /output /
