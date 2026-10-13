fn main() {
    if let Err(error) = lumendesk_cli::run(std::env::args().skip(1)) {
        eprintln!("lumendesk: {error}");
        std::process::exit(1);
    }
}
