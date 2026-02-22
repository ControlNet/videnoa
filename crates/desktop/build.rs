fn main() {
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/icon.png");
    println!("cargo:rerun-if-changed=icons/icon-32.png");
    println!("cargo:rerun-if-changed=icons/icon-64.png");
    println!("cargo:rerun-if-changed=icons/icon-128.png");
    println!("cargo:rerun-if-changed=icons/icon-256.png");

    tauri_build::build();
}
