//! A linker wrapper that throttles the link step with a file-locks to control concurrency. 
//! 
//! The main purpose is to prevent Out-of-Memory kills due to linking consuming too much RAM.
//! This only affects linking and is an addition to the `--jobs` option of cargo.
//! 
//! # Configuration
//!
//! All configuration is via environment variables read on each invocation:
//!
//! | Variable          | Required | Default                                 | Purpose                                                                  |
//! | ----------------- | -------- | --------------------------------------- | ------------------------------------------------------------------------ |
//! | `RLC_LINKER`      | yes      | —                                       | Path or name of the real linker to exec (e.g. `clang`, `link.exe`).      |
//! | `RLC_CONCURRENCY` | no       | `1`                                     | Maximum number of concurrent link invocations.                           |
//! | `RLC_LOCK_DIR`    | no       | `$CARGO_TARGET_DIR/.rlc-locks` or `$TMPDIR/rlc-$USER` | Directory holding the per-slot lock files. Created on demand. |
//! | `RLC_VERBOSE`     | no       | unset                                   | If set to a non-empty value other than `0`, log slot wait/timing to stderr. |
//!
//! # Wiring it into cargo
//!
//! Recommended: per-shell or per-CI via `CARGO_TARGET_<TRIPLE>_LINKER` so no
//! `~/.cargo/config.toml` edit is needed.
//!
//! ```sh
//! # Example for the x86_64-unknown-linux-gnu target; adjust the target-triple.
//! export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=/path/to/rust-linker-concurrency
//! export RLC_LINKER=clang
//! export RLC_CONCURRENCY=2
//! cargo build
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    match rust_linker_concurrency::run() {
        Ok(c) => ExitCode::from(c),
        Err(e) => {
            eprintln!("rust-linker-concurrency: {e}");
            ExitCode::FAILURE
        }
    }
}
