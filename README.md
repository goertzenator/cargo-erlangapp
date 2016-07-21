# cargo-erlangapp
A cargo subcommand for building Rust crates embedded in an Erlang application.  All crates in the `crates` directory get compiled and placed into `priv/crates`.  Below is an example of an Erlang application with the artifacts from two crates placed into `priv/crates`:

```
myapp/
    Makefile
    ebin/
    src/
    crates/
        foo_nif/
            Cargo.toml
            ...
        bar_port/
            Cargo.toml
             ...
    priv/
        crates/
            foo_nif/
                libfoo_nif.so
            bar_port/
                bar_port
```

cargo-erlangapp is intended to be installed and used automatically by Erlang build system like `erlang.mk` and `rebar3`, but may also be used manually.

The Erlang application [`find_crate`](https://github.com/goertzenator/find_crate) assists in locating Rust artifacts in `priv/crates`.

## Installation
```
cargo install cargo-erlangapp
```

## Usage
```
Usage:
        cargo-erlangapp build [cargo rustc args]
        cargo-erlangapp clean [cargo clean args]
        cargo-erlangapp test [cargo test args]
```

## Under the Hood
`cargo-erlangapp` takes care of a few wrinkles when compiling Rust code for Erlang:
- OS X requires special link flags when compiling dylibs (ie, NIF modules) for Erlang.  To do that, `cargo-erlangapp` has to read the JSON manifest to identify all the targets and compile each individually and applying special flags to just dylibs.
- On OS X, the Erlang dylib loader requires files named differently than what Rust produces.

