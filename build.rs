fn main() {
    if std::env::var_os("CARGO_FEATURE_CUDA").is_some() {
        let target_arch =
            std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "unknown".to_string());
        if target_arch != "x86_64" {
            eprintln!(
                "error: the `cuda` feature is only supported when targeting x86_64 due to NVIDIA CUDA requirements.\n       detected target architecture: `{target_arch}`\n       tip: rebuild without `--features cuda` on this machine."
            );
            std::process::exit(1);
        }

        let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        if target_os == "macos" {
            println!(
                "cargo:warning=CUDA binaries are not available on macOS targets. The `cuda` feature is disabled."
            );
        }
    }
}
