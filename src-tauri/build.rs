fn main() {
    // The UI is embedded at compile time, so a new UI build must rebuild this crate.
    println!("cargo:rerun-if-changed=../ui/dist");
    tauri_build::build()
}
