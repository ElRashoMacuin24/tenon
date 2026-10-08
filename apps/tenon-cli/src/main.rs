//! Headless Tenon command line.
#![forbid(unsafe_code)]

fn main() {
    println!("tenon-cli {}", env!("CARGO_PKG_VERSION"));
}
