#!/bin/sh

set -e

PATH=$PATH:$HOME/.cargo/bin
if [[ -n "${DEVELOPER_SDK_DIR:-}" ]]; then
  # Assume we're in Xcode, which means we're probably cross-compiling.
  # In this case, we need to add an extra library search path for build scripts and proc-macros,
  # which run on the host instead of the target.
  # (macOS Big Sur does not have linkable libraries in /usr/lib/.)
  export LIBRARY_PATH="${DEVELOPER_SDK_DIR}/MacOSX.sdk/usr/lib:${LIBRARY_PATH:-}"
fi

# If you want your build to run faster, add a "--targets x86_64-apple-ios" for just using the ios simulator.
if [ -n "${IOS_TARGETS}" ]; then
    cargo lipo --targets "${IOS_TARGETS}"
else
    cargo lipo
fi
