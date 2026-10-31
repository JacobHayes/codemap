//! Digests the web build so a changed UI recompiles the embed: the build cache can otherwise
//! restore an embed older than `web/dist`.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::{fs, io};

fn hash_dir(dir: &Path, hasher: &mut DefaultHasher) -> io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        path.file_name().hash(hasher);
        if path.is_dir() {
            hash_dir(&path, hasher)?;
        } else {
            fs::read(&path)?.hash(hasher);
        }
    }
    Ok(())
}

fn main() {
    let dist = Path::new("../../web/dist");
    // The embed needs the folder to exist even before the first web build.
    fs::create_dir_all(dist).expect("web/dist is creatable in the workspace");
    let mut hasher = DefaultHasher::new();
    hash_dir(dist, &mut hasher).expect("web/dist is readable");
    println!("cargo:rerun-if-changed=../../web/dist");
    println!(
        "cargo:rustc-env=CODEMAP_WEB_DIGEST={:016x}",
        hasher.finish()
    );
}
