//! Links the prebuilt static libraries of `DuckDB` that `scripts/duckdb` fetches (the `duckdb` crate links
//! `libduckdb_static.a` itself): the core-functions archive, the registration of it, and the C++
//! runtime the archive leaves to its final link.

use std::{env, path::Path};

fn main() {
    println!("cargo:rerun-if-env-changed=DUCKDB_LIB_DIR");
    println!("cargo:rerun-if-changed=register_extensions.c");
    let lib_dir = env::var("DUCKDB_LIB_DIR").expect("DUCKDB_LIB_DIR comes from .cargo/config.toml");
    assert!(
        Path::new(&lib_dir).join("libduckdb_static.a").exists(),
        "DuckDB's static libraries are missing from {lib_dir}: run scripts/duckdb"
    );

    println!("cargo:rustc-link-search=native={lib_dir}");
    println!("cargo:rustc-link-lib=static=core_functions_extension");
    let runtime = match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => "c++",
        _ => "stdc++",
    };
    println!("cargo:rustc-link-lib={runtime}");

    // Whole-archive keeps the constructor, which nothing references.
    let out_dir = env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    cc::Build::new()
        .file("register_extensions.c")
        .cargo_metadata(false)
        .compile("register_extensions");
    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=static:+whole-archive=register_extensions");
}
