ARG CROSS_BASE_IMAGE
FROM $CROSS_BASE_IMAGE

ENV PKG_CONFIG_ALLOW_CROSS=1
ENV PKG_CONFIG_PATH=/usr/lib/arm-linux-gnueabihf/pkgconfig/

RUN dpkg --add-architecture armhf && \
    apt-get update && \
    apt-get install libasound2-dev:armhf -y && \
    apt-get install libjack-jackd2-dev:armhf libjack-jackd2-0:armhf -y
# TODO: now the cross-rs is based on ubuntu:20.04, so it does not contain pipewire-0.3-dev
