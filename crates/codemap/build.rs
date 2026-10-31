//! Recompiles the embed when `web/dist` changes: the embed reads the folder itself, so the
//! compiler would not see a changed file as an input.

fn main() {
    // The embed needs the folder to exist even before the first web build.
    std::fs::create_dir_all("../../web/dist").expect("web/dist is creatable in the workspace");
    println!("cargo::rerun-if-changed=../../web/dist");
}
