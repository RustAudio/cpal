#!/bin/bash
set -e
(echo 'wait for pipewire') || exit 1
(pw-cli ls 0 | grep --quiet 'id 0, type PipeWire:Interface:Core/4') || exit 1
(echo 'wait for wireplumbler') || exit 1
(wpctl info | grep --quiet 'WirePlumber') || exit 1
(echo 'wait for PipeWire Pulse') || exit 1
(pactl info | grep --quiet "$PULSE_RUNTIME_PATH/native") || exit 1
(echo 'wait for test-sink') || exit 1
(pactl set-default-sink 'test-sink') || exit 1
(wpctl status | grep --quiet 'test-sink') || exit 1
(echo 'wait for test-source') || exit 1
(pactl set-default-source 'test-source') || exit 1
(wpctl status | grep --quiet 'test-source') || exit 1