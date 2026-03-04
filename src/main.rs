use std::process::ExitCode;

use rccgen::RccGen;

fn main() -> ExitCode {
    let mut generator = match RccGen::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("rccgen: Error initializing: {}", e);
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = generator.run() {
        eprintln!("rccgen: Error: {}", e);
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
