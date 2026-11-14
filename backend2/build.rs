fn main() {
    for var in [
        "GNUPLEX_SOURCE_HASH",
        "GNUPLEX_PLATFORM",
        "GNUPLEX_COMPILER_VERSION",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
}
