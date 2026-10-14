fn main() {
    match silod::entry(&std::env::args().collect::<Vec<_>>()) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("silod: {error}");
            std::process::exit(1);
        }
    }
}
