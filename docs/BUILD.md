# How to build and run Fomos in an emulator

Draft version

## Dependencies

- rust
- qemu

### Rust

[Install Rust](https://www.rust-lang.org/tools/install)

Then, the nightly version of the compiler is needed:

```
rustup toolchain install nightly
```

Additionnal components might be compiler specific, switch to the nightly version before installing them.

```
rustup default nightly
```

Fomos needs to be cross compiled. It is x86_64 only for now, add the appropriate compiler target:

```
rustup target add x86_64-unknown-none
```

On some system you need specific components. Example for my macbook air 2015.

```
rustup component add rust-src --toolchain nightly-x86_64-apple-darwin
```

```
rustup component add llvm-tools
```

The rust compiler will guide you with errors in the terminal while building.

### QEMU

[Install QEMU](https://www.qemu.org/)

You might need to compile it yourself with the SDL option. You can also remove SDL in the next step otherwise.

# Build and run

In the root of the project, execute `./build.sh`.
It should build all the independent apps one by one, and finally build the OS, and run it in qemu.

There are some qemu launch parameters in ./bootloader/src/main.rs

By default they suppose you have a KVM capable machine, and qemu with SDL.

If you do not have qemu with SDL replace

```rust
cmd.arg("-device").arg("virtio-vga-gl");
cmd.arg("-display").arg("sdl,gl=on");
```

with

```rust
cmd.arg("-device").arg("virtio-vga");
```

If you do not have KVM, remove the --enable-kvm option. This makes the emulation extremely slow.
