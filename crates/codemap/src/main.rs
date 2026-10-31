//! The codemap binary. All behaviour lives in the library half of this crate.

use std::process::ExitCode;

fn main() -> ExitCode {
    codemap::run()
}
