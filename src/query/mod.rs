#[cfg(feature = "extension")]
pub mod error;
#[cfg(feature = "extension")]
pub mod explain;
#[cfg(feature = "extension")]
pub mod table_function;

// Pure wire-format / SQL-shape helpers, always compiled so they are covered by
// the default `cargo test` / clippy / coverage runs even though the FFI
// entrypoints that call them are `extension`-gated (TC-8).
pub mod wire;
