use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/branding/pordo.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/branding/pordo.ico")
            .set("ProductName", "Pordo")
            .set("FileDescription", "Pordo")
            .set("OriginalFilename", "pordo.exe")
            .compile()
            .expect("compile Windows application icon and version resources");
    }
}
