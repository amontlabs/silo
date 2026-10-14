fn main() -> std::io::Result<()> {
    let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join("silo-api.schema.json");
    if let Some(parent) = schema_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    silo_api::write_schema(schema_path)
}
