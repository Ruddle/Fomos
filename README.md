# Fomos

Experimental OS, built with Rust

![Demo](assets/demo.mp4)

# Why

I wanted to experiment with Non-Unix OS ideas.

Exo-kernels are interesting, but it is mostly a theory. This project helps me understand the challenges involved in that pattern.

OS development is extremely hard, Rust makes it more bearable.

# Features

- Has a graphical output
- Load and run concurrent apps
- All programs run in an async loop
- Support Virtio mouse and keyboard (drivers are async tasks)
- Cooperative scheduling (apps yield control as much as possible)
- _Nearly support Virgl_

There is 4 examples of apps in this repo named app\_\*

# What is unique

# Building

On a linux, run

```sh
./build.sh
```

_You might need rust nightly_

# Credit

Heavily inspired by [Philipp Oppermann's blog](https://os.phil-opp.com/).

Thanks to [darbysauter](https://github.com/darbysauter/myOS) for the advice given.
