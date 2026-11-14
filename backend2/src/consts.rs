// Build metadata, mirroring backend/consts/version.go and make.py's Go
// ldflags. The names are kept for API compatibility with existing clients.
pub const VERSION: &str = "1791246758";

pub const SOURCE_HASH: &str = match option_env!("GNUPLEX_SOURCE_HASH") {
    Some(v) => v,
    None => "",
};

pub const PLATFORM: &str = match option_env!("GNUPLEX_PLATFORM") {
    Some(v) => v,
    None => "",
};

pub const COMPILER_VERSION: &str = match option_env!("GNUPLEX_COMPILER_VERSION") {
    Some(v) => v,
    None => "",
};
