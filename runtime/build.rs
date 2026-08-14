use std::{env, fs, path::PathBuf};

// Fills in runtime_version.rs.template's {{SPEC_VERSION}} placeholder from
// this crate's own Cargo.toml version (release-please bumps
// `workspace.package.version`, which this crate inherits via
// `version.workspace = true`) and writes the result to OUT_DIR, so
// spec_version can never drift the way a checked-in generated file could.

fn write_spec_version() {
    println!("cargo:rerun-if-changed=src/runtime_version.rs.template");

    let major = parse_version_component("CARGO_PKG_VERSION_MAJOR");
    let minor = parse_version_component("CARGO_PKG_VERSION_MINOR");
    let patch = parse_version_component("CARGO_PKG_VERSION_PATCH");
    let spec_version = major * 1_000_000 + minor * 10_000 + patch * 100;

    let template = fs::read_to_string("src/runtime_version.rs.template")
        .expect("runtime/src/runtime_version.rs.template should exist");
    let generated = template.replace("{{SPEC_VERSION}}", &spec_version.to_string());

    let dest = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR set by cargo; qed"))
        .join("runtime_version.rs");
    fs::write(dest, generated).expect("failed to write generated runtime_version.rs to OUT_DIR");
}

fn parse_version_component(var: &str) -> u32 {
    let value: u32 = env::var(var)
        .unwrap_or_else(|_| panic!("{var} is not set by cargo"))
        .parse()
        .unwrap_or_else(|_| panic!("{var} is not a valid u32"));
    assert!(value <= 99, "{var}={value} exceeds the two-digit spec_version format");
    value
}

#[cfg(all(feature = "std", feature = "metadata-hash"))]
fn main() {
    write_spec_version();
    substrate_wasm_builder::WasmBuilder::init_with_defaults()
        .enable_metadata_hash("UNIT", 12)
        .build();
}

#[cfg(all(feature = "std", not(feature = "metadata-hash")))]
fn main() {
    write_spec_version();
    substrate_wasm_builder::WasmBuilder::build_using_defaults();
}

/// The wasm builder is deactivated when compiling
/// this crate for wasm to speed up the compilation.
#[cfg(not(feature = "std"))]
fn main() {
    write_spec_version();
}
