[package]
name = "__CRATE_NAME__"
version = "0.1.0"
edition = "2024"
publish = false

[package.metadata.aria]
tier = "__TIER__"

[lib]
crate-type = ["cdylib"]

[dependencies]
__DEPENDENCY_LINE__

[profile.release]
opt-level = "s"
lto = true
strip = true
codegen-units = 1
panic = "abort"

[workspace]
