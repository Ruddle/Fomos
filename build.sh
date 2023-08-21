#!/bin/bash
cd app_background 
RUSTFLAGS="-C relocation-model=pie -C link-arg=-pie" cargo build --release
cd ..
cd app_cursor 
RUSTFLAGS="-C relocation-model=pie -C link-arg=-pie" cargo build --release
cd ..
cd app_console
RUSTFLAGS="-C relocation-model=pie -C link-arg=-pie" cargo build --release
cd ..
cd bootloader
cargo run --release
