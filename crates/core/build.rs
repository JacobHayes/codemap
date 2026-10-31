//! Links the prebuilt static libraries of `DuckDB` that `scripts/duckdb` fetches (the `duckdb` crate links
//! `libduckdb_static.a` itself): the core-functions archive, the registration of it, and the C++
//! runtime the archive leaves to its final link. It also digests the sources of this crate and of
//! `codemap-lang` into the index's build id, so the id changes exactly when extraction may.

use std::{env, fs, io, path::Path};

/// Feeds every file under `dir` (relative path, then bytes) into `hasher` in name order.
fn hash_dir(root: &Path, dir: &Path, hasher: &mut blake3::Hasher) -> io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            hash_dir(root, &path, hasher)?;
        } else {
            let relative = path.strip_prefix(root).expect("walked from the root");
            hasher.update(relative.to_string_lossy().as_bytes());
            hasher.update(&fs::read(&path)?);
        }
    }
    Ok(())
}

fn emit_build_id() {
    let mut hasher = blake3::Hasher::new();
    for dir in [".", "../lang"] {
        println!("cargo::rerun-if-changed={dir}");
        hash_dir(Path::new(dir), Path::new(dir), &mut hasher)
            .expect("the core and lang crate folders are readable");
    }
    // The lockfile pins the grammars and tokenizer that extraction runs on.
    println!("cargo::rerun-if-changed=../../Cargo.lock");
    hasher.update(&fs::read("../../Cargo.lock").expect("Cargo.lock is readable in the workspace"));
    println!(
        "cargo::rustc-env=CODEMAP_INDEX_BUILD={}",
        hasher.finalize().to_hex()
    );
}

fn main() {
    emit_build_id();
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
