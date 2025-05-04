use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let target_dir = Path::new(&out_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    // Create plugins directory if it doesn't exist
    let plugins_dir = target_dir.join("plugins");
    fs::create_dir_all(&plugins_dir).unwrap();

    // Copy the built library to plugins directory
    let lib_name = env::var("CARGO_PKG_NAME").unwrap();
    let src = target_dir
        .join("release")
        .join(format!("lib{}.so", lib_name));
    let dst = plugins_dir.join(format!("lib{}.so", lib_name));

    // Only try to copy if the source file exists
    if src.exists() {
        if let Err(e) = fs::copy(&src, &dst) {
            println!("cargo:warning=Failed to copy library: {}", e);
        }
    }
}
