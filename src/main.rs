fn main() {
    if let Err(err) = synto::cli::run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
