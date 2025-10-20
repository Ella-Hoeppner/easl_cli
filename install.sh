#!/bin/bash

set -e
cargo build --release
mkdir -p bin

# Detect platform and handle .exe extension on Windows
if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" || "$OSTYPE" == "cygwin" ]]; then
    cp target/release/easl.exe bin/easl.exe
else
    cp target/release/easl bin/easl
fi
