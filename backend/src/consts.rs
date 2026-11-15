// Build metadata, mirroring backend/consts/version.go and make.py's Go
// ldflags. The names are kept for API compatibility with existing clients.
//
// These are compile-time constants baked into the binary by build.rs. The
// environment is only read while compiling; it cannot override them at
// runtime.
pub const VERSION: &str = "1791246758";

pub const SOURCE_HASH: &str = env!("GNUPLEX_SOURCE_HASH");

pub const PLATFORM: &str = env!("GNUPLEX_PLATFORM");

pub const COMPILER_VERSION: &str = env!("GNUPLEX_COMPILER_VERSION");
