#!/bin/bash
cargo build --release --bin serial-keel --target x86_64-unknown-linux-musl
cp target/x86_64-unknown-linux-musl/release/serial-keel bin/serial-keel

