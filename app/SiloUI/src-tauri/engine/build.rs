// The engine reads the GitHub App's configuration at compile time
// (`option_env!` in src/github.rs). Load it the same way the desktop crate does,
// from the build environment or app/SiloUI/github-build.local.json, so an engine
// built on its own (`cargo build -p silod`) can connect GitHub too.
#[path = "../github_build.rs"]
mod github_build;

fn main() {
    let config =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../github-build.local.json");
    github_build::watch(&config);
    println!("cargo:rerun-if-changed=../github_build.rs");
    let configured = config.exists()
        || github_build::KEYS
            .iter()
            .any(|key| std::env::var_os(key).is_some());
    if !configured {
        // Contributors without a GitHub App still get a working engine; GitHub
        // reports "not configured in this build" at runtime.
        println!("cargo:warning=No GitHub App configuration: this engine cannot connect GitHub. See docs/SiloUI-RELEASES.md.");
        return;
    }
    // A partial or invalid configuration fails the build, as in the desktop crate.
    let values = github_build::configuration(&config, |key| std::env::var(key).ok())
        .unwrap_or_else(|message| panic!("{message}"));
    for (key, value) in values {
        println!("cargo:rustc-env={key}={value}");
    }
}
