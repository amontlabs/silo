fn main() -> std::io::Result<()> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("methods")
        .join("requests.rs");
    std::fs::write(path, silo_api::methods::request::requests_source())
}
