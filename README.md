> You can support this night time project by hiring me for a day time job !

# Fomos

Experimental OS, built with Rust

https://github.com/Ruddle/Fomos/assets/14235713/3ee75d5e-5ebe-4cc1-b267-8b73337ee157

**Fun fact**: there are 3 apps running in the video. A background app, a cursor app, and a console app.

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
- No context switches once booted
- _Nearly support Virgl_ ™

There is 5 examples of apps in this repo named `app_*`, some in Rust, one in C.
The kernel is in `bootloader`.

# What is unique

The signature of an app in Fomos:

```rust
pub extern "C" fn _start(ctx: &mut Context) -> i32
```

Apps do not need a standard library, any OS functionality is given to the app through the _Context_.

the _Context_ is mostly a pointer to a bag of kernel functionnalities

```rust
pub extern "C" fn
```

In Fomos, an app is really just a **function**. There is nothing else ! This is a **huge** claim. An executable for a Unix or Windows OS is extremely complex compared to a freestanding function.

`<rant>`

It is out a frustration for all my Glibc problems during day job dev on linux that I chose to try this approach.

I want a flat contract between an app and the OS. So what if an app was a function ? The contract is then **only** the explicit argument type.

In Unix, an app has to know the target OS, but also what standard library it uses, that is 2 levels of indirections. Sometimes the os level has a conflict, sometimes the standard library level has a conflict, and sometimes I just don't have the level to understand why something doesn't work. I merely know it is related.

`</rant>`

I am trying to know if it is possible to have an OS-App ecosystem that does not suppose **ANY** **implicit** configuration. An app would **JUST** have to handle its explicit `start` _context_ argument.

_Context_ gives any OS function necessary, think alloc, free, access to a framebuffer, or any hardware, any system calls etc.

That way, apps could be freestanding, and compatible on multiple OS.

### More about Context

Here is the _Context_ for the last version of this OS

```rust
#[repr(C)]
pub struct Context<'a, T> {
    pub version: u8,
    pub start_time: u64,
    pub log: extern "C" fn(s: *const u8, l: u32),
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
    log: extern "C" fn(s: *const u8, l: u32),
    pid: u64,
    fb: FB<'a>,
}
```

Meaning Fomos already handles gracefully Apps designed for a much older version of itself. As long as the OS stays compatible with the old stuff in the context, it can add new functionalities for other App by just appending to the context the new functions (here calloc, cdalloc, store, and input).

`app_test` precedes the dynamic allocation age !

Could that pattern work in the long term ?

### How about system calls

None. Lets try to put everything into _Context_ functions. No voodoo cpu instruction magic.

> But how do you give back control to the OS ?

Just

```rust
return;
```

> How do you sleep, or wait asynchronously ?

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

There are a lot of questions without answer yet, but by now you might be curious, what if all the question had an answer in the pattern ? It looks like it could actually work (with a lot of work).

# Advantages

A lot of stuff comes free once you accept the premises.

#### Sandboxing, instrumentation, debugging

Every functionnality and side effect given to an app goes explicitely through the _Context_. The _Context_ is just a struct, we can wrap or replace anything in it.
Lets instrument an app we'll call `special_app`. Over simplification :

```rust
loop {
    for normal_app in normal_apps.iter_mut() {
        app._start(Context::new(alloc,..));
    }
    // special_app alloc instrumentation
    fn alloc_log(..){log("allocation detected!"); return alloc(..);}
    special_app._start(Context::new(alloc_log,..));
}
```

#### Restart, sleep, change of hardware

An app memory lives in its context. The stack is fleeting. It is reset after each yield and doesn't mean much in Fomos.
Since the _Context_ is explicit, it can be stored. A restart _can_ be made completely transparent to an app.

Pseudo code:

```rust
//kernel just started
...
let app = App::new(..);
let ctx = disk.load(app.id).unwrap_or(Context::new(..));
loop{
    app._start(ctx);
    if restart_request{
        disk.save(app.id, ctx)
        break;
    }
}
//handle restart
...
```

Quickload and quicksave of an app complete state is trivial.
Note that some change of hardware could make an app bug. It would be a problem if it was transparent. However, it could be made opaque and obvious, in an opt-in manner, again through the _Context_.

# Disadvantages

### Security

Right now it is not implemented, any app can casually check the ram of another app ^^. This is going to be a hard problem to solve. I have plans to have data security without context switch, and without giving every damn app its own virtual memory stack.

### Cooperative vs preemptive scheduling

The argument that a cooperative scheduling is doomed to fail is overblown. Apps are already very much cooperative.
For proof, run a version of that on your nice preemptive system :

```js
while(true){
  new Thread( () => {
    fs.writeFile("/home/"+randomString(),randomString())
    malloc(randomInt())
    curl("http://"+randomString()+".com")
  }
}
```

- Blender does a compelling impression of that when you increase the level of details one too many times. Might fill your swap and crash unsaved work on other apps.
- Badly written Webgl websites crash my gpu driver.

Not only is preemptive scheduling not enough, IMO it is not necessary. Also it is a spectrum. A system can be optimistically cooperative, and turn preemptive pessimistically.

However the ecosystem is made for preemptive OS. There is friction in doing things differently.

# Missing

- Permanent storage (should be easy since virtio is already implemented)
- Gpu support (virgl wip)
- Networking
- A nice abstraction for apps to share data and functionnalities between themselves

The rest should live in userland.

# Building

run

```sh
./build.sh
```

_You might need rust nightly, gcc, qemu with virgl & sdl flag_

[More info here](/docs/BUILD.md)

# Credit

Heavily inspired by [Philipp Oppermann's blog](https://os.phil-opp.com/).

Thanks to [darbysauter](https://github.com/darbysauter/myOS) for the advice given.
