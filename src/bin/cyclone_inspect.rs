//! `cyclone-inspect` - decode a packet through a named schema.

use std::process::ExitCode;

use cyclonec::inspect;

fn main() -> ExitCode {
    let options = match inspect::parse(std::env::args_os().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            print!("{}", inspect::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("cyclone-inspect: {message}\n\n{}", inspect::USAGE);
            return ExitCode::from(2);
        }
    };

    match inspect::run(&options) {
        Ok(report) => {
            print!("{report}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("cyclone-inspect: {message}");
            ExitCode::FAILURE
        }
    }
}
