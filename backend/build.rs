use std::env;

fn main() {
    // Make.py supplies these build metadata values. Bake them into the binary
    // here so `consts` can use `env!` and they can never be changed by the
    // environment at runtime. Missing values stay empty for direct cargo
    // builds; Cargo re-runs this script when any of them changes.
    for var in [
        "GNUPLEX_SOURCE_HASH",
        "GNUPLEX_PLATFORM",
        "GNUPLEX_COMPILER_VERSION",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
        let value = env::var(var).unwrap_or_default();
        println!("cargo:rustc-env={var}={value}");
    }
}
