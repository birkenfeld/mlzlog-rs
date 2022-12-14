mlzlog in Rust
==============

[![Latest Version](https://img.shields.io/crates/v/mlzlog.svg)](https://crates.io/crates/mlzlog)

[Documentation](https://docs.rs/mlzlog/)

This is a Rust crate that provides a [`log4rs`] configuration with custom
appenders that logs like the [`mlzlog`] Python package.

[`log4rs`]: https://github.com/sfackler/log4rs
[`mlzlog`]: http://pypi.python.org/pypi/mlzlog


Installation
============

This crate works with Cargo and can be found
on [crates.io](https://crates.io/crates/mlzlog) with a `Cargo.toml` like:

```toml
[dependencies]
log = "0.4"
mlzlog = "0.7"
```

Minimum supported Rust versions is 1.41.1.

Usage
=====

Initialize logging at the beginning of your program and then use the
macros from the `log` crate. Example:

```rust
use log::info;

fn main() {
    mlzlog::init("/path/to/base", "myapp", mlzlog::Settings { ... });

    info!("starting up");
}
```
