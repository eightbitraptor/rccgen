use rccgen::RccGen;

fn main() {
    let mut generator = match RccGen::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("rccgen: Error initializing: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = generator.run() {
        eprintln!("rccgen: Error: {}", e);
        std::process::exit(1);
    }
}
