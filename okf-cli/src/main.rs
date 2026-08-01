//! Composition root. Wires adapters into usecases -- the one place that names both.
fn main() {
    let _ = okf_fs_bundle::FsBundle;
    let _ = okf_yaml_serde::YamlSerde;
    println!("okf-cli scaffold");
}
