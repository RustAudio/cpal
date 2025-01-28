#!/bin/bash
mkdir --parents ${PIPEWIRE_RUNTIME_DIR}
mkdir --parents /etc/pipewire/pipewire.conf.d/
cp /root/virtual.nodes.conf /etc/pipewire/pipewire.conf.d/virtual.nodes.conf
supervisord -c /root/supervisor.conf
rm --force --recursive ${PIPEWIRE_RUNTIME_DIR}