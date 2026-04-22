fn main() {
    if let Err(err) = flowd_rs::server::run() {
        eprintln!("Server failed: {}", err);
        std::process::exit(1);
    }
}
