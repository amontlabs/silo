fn main() {
    println!("cargo:rerun-if-env-changed=SILOD_PRODUCTION_CODE_REQUIREMENT");
    println!("cargo:rerun-if-env-changed=SILOD_DEVELOPMENT_CODE_REQUIREMENT");
}
