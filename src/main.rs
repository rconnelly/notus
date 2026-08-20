fn main() {
    if let Err(err) = notus::cli::run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
