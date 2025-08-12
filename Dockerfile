# Take in "PLATFORM" or default to the latest ubuntu version
ARG PLATFORM
FROM ${PLATFORM:-ubuntu:latest}

ENV PKG_CONFIG_ALLOW_CROSS=1
ENV PKG_CONFIG_PATH=/usr/lib/arm-linux-gnueabihf/pkgconfig/

# Install audio essentials
RUN dpkg --add-architecture armhf && \
    apt-get update && \
    apt-get install libasound2-dev:armhf -y && \
    apt-get install libjack-jackd2-dev:armhf libjack-jackd2-0:armhf -y


# Install curl
RUN apt-get install -y curl

# Install and setup Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y

ENV PATH="/root/.cargo/bin:${PATH}"

# Install "build-essential" to allow the 'cc' crate.
RUN apt-get install -y build-essential
