fn main() {
    println!("cargo::rerun-if-env-changed=BWRAP_PATH");
    println!("cargo::rerun-if-env-changed=BETTERLEAKS_PATH");
    println!("cargo::rerun-if-env-changed=OPENCODE_PATH");

    let is_release = std::env::var("PROFILE").unwrap_or_default() == "release";

    if is_release {
        if std::env::var("BWRAP_PATH").is_err() {
            println!("cargo::warning=BWRAP_PATH not set at build time. Binary will require bwrap_path in config YAML at runtime.");
        }
        if std::env::var("BETTERLEAKS_PATH").is_err() {
            println!("cargo::warning=BETTERLEAKS_PATH not set at build time. Binary will require betterleaks_path in config YAML at runtime.");
        }
        if std::env::var("OPENCODE_PATH").is_err() {
            println!("cargo::warning=OPENCODE_PATH not set at build time. Binary will require agent binary in config YAML at runtime.");
        }
    }
}
