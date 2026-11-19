//! pulsr: a ridge visualiser in the terminal for whatever the computer is playing.
//! A capture thread analyses the system's output into frames of band levels (`spectrum`);
//! the loop in `term` reads them against the audible clock and draws the ridge.

// `!(x > 0.0)` is deliberate: unlike `x <= 0.0` it is also true for NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

mod audio;
mod bands;
mod clock;
mod filter_bank;
mod level_mapper;
mod ridge;
mod spectrum;
#[cfg(unix)]
mod term;

#[cfg(unix)]
fn run(args: Vec<String>) -> i32 {
    match term::Options::parse(args.into_iter().skip(1)) {
        Ok(options) => term::run(options),
        Err(error) => {
            if !error.is_empty() {
                eprintln!("pulsr: {error}\n");
            }
            eprintln!("{}", term::USAGE);
            if error.is_empty() { 0 } else { 2 }
        }
    }
}

#[cfg(not(unix))]
fn run(_: Vec<String>) -> i32 {
    eprintln!("pulsr: not built for this platform yet");
    2
}

fn main() {
    std::process::exit(run(std::env::args().collect()));
}
