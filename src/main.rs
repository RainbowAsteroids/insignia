use std::env;
use std::process::exit;

use insignia;

fn print_err(e: insignia::Error) -> ! {
    println!("{}", e.error_str);
    exit(e.error_code);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let config = insignia::Config::new(&args[1..], &args[0]);

    let config = match config {
        Ok(c) => c,
        Err(e) => print_err(e)
    };

    if let Err(e) = config.exec() { print_err(e) }
}
