use std::env;

fn main() {
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());

    let version = if profile == "release" {
        // Release: use Cargo.toml version, only rebuild when Cargo.toml changes
        println!("cargo:rerun-if-changed=Cargo.toml");
        env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".to_string())
    } else {
        // Debug: use current datetime, always rebuild (no rerun-if-changed)
        chrono::Utc::now().format("%Y%m%d%H%M%S").to_string()
    };

    println!("cargo:rustc-env=BUILD_VERSION={}", version);
}
