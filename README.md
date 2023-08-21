# Fomos

Experimental OS, built with Rust

https://github.com/Ruddle/Fomos/assets/14235713/ec25bfd5-76b7-4808-85c8-9d7465d02544

**Fun fact**: there are 4 apps running in the video. A background app, a cursor app, and 2 blurry window apps.

# Why

I wanted to experiment with Non-Unix OS ideas.

Exo-kernels are interesting, but it is mostly a theory. This project helps me understand the challenges involved in that pattern.

OS development is extremely hard, Rust makes it more bearable.

# Features

- Has a graphical output
- Dynamic allocation
- Load and run concurrent apps
- All apps run in an async loop
- Support Virtio mouse and keyboard (drivers are async tasks)
- Cooperative scheduling (apps yield control as much as possible)
- No context switch once booted
- _Nearly support Virgl_

There is 4 examples of apps in this repo named `app_*`.
The kernel is in `bootloader`.

# What is unique

The signature of an app in Fomos:

```rust
pub extern "C" fn _start(ctx: &mut Context) -> i32
```

App do not need a standard library, any OS functionality is given to the app through the _Context_.

the _Context_ is mostly a pointer to a bag of

```rust
pub extern "C" fn
```

In Fomos, an app is really just a function. There is nothing else ! This is a **huge** claim. An executable for a Unix or Windows OS is extremely complex compared to a freestanding function.

It is out a frustration for all my Glibc problems during day job dev on linux that I chose to try this approach.

I want a flat contract between an app and the OS. So what if an app was a function ? The contract is then **only** the explicit argument type.

In Unix, an app has to know the target OS, but also what standard library it uses, that is 2 levels of indirections. Sometimes the os level has a conflict, sometimes the standard library level has a conflict, and sometimes I just don't have the level to understand why something doesn't work. I merely know it is related.

I am trying to know if it is possible to have an OS-App ecosystem that does not suppose **ANY** **implicit** configuration. I want a world where an app **JUST** has to handle its `start` _context_ argument.

_Context_ gives any OS functions necessary, think alloc, free, access to a framebuffer, or any hardware, any system calls etc.

That way, apps could be freestanding, and compatible on multiple OS.

### More about Context

Here is the Context for the last version of this OS

```rust
#[repr(C)]
pub struct Context<'a, T> {
    pub version: u8,
    pub start_time: u64,
    pub log: extern "C" fn(s: &str),
    pub pid: u64,
    pub fb: FB<'a>,
    pub calloc: extern "C" fn(usize, usize) -> *mut u8,
    pub cdalloc: extern "C" fn(*mut u8, usize, usize),
    pub store: &'a mut Option<Box<T>>,
    pub input: &'a Input,
}
```

Note that `app_test` for instance, uses an old version of the _Context_, and still works on the newer version of the OS

Old Context used by `app_test`:

```rust
#[repr(C)]
pub struct Context<'a> {
    pub version: u8,
    start_time: u64,
    log: extern "C" fn(s: &str),
    pid: u64,
    fb: FB<'a>,
}
```

Meaning Fomos already handles gracefully Apps designed for a much older version of itself. As long as the OS stays compatible with the old stuff in the context, it can add new functionalities for other App by just appending to it the new functions (here calloc, cdalloc, store, and input).

`app_test` precedes the dynamic allocation age !

Could that pattern work in the long term ?

### How about system calls

None. Lets try to put everything into _Context_ functions. No voodoo cpu instruction magic.

> But how do you give back control to the OS ?

Just

```rust
return;
```

Apps are **cooperative** in Fomos, They can just return (which would exit permanently an app on a classic OS), and assume that they are gonna be called through their only function `start` again soon, maybe even instantly if the "system call" works that way.

> But an app loses all RAM data everytime it yields that way !

No, an app can store anything it wants in Context.store during its execution, and get it back every `start` call. The OS keeps everything in RAM (on the heap). The stack itself is "reset". But it is not more "reset" than it is after any function execution in a normal program. You don't lose anything. In Fomos, apps are merely a single function called multiple times!

Over simplification of the kernel loop:

```rust
loop {
    for app in apps.iter_mut() {
        app._start(Context::new(...));
    }
}
```

I know, you are going to have a question I can't answer yet, but by now you might be curious, what if all the question had an answer in the pattern ? It looks like it could actually work (with a lot of work).

### Security

Right now it is not implemented, any app can casually check the ram of another app ^^. This is going to be a hard problem to solve. I have plans to have data security without context switch, and without giving every damn app its own virtual memory stack.

# Missing

- Permanent storage (should be easy since virtio is already implemented)
- Gpu support (virgl wip)
- Networking

The rest should live in userland.

# Building

On a linux, run

```sh
./build.sh
```

_You might need rust nightly_

# Credit

Heavily inspired by [Philipp Oppermann's blog](https://os.phil-opp.com/).

Thanks to [darbysauter](https://github.com/darbysauter/myOS) for the advice given.
