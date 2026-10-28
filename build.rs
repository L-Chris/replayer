use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/branding/replayer.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/branding/replayer.ico")
            .set("ProductName", "replayer")
            .set("FileDescription", "replayer")
            .set("OriginalFilename", "replayer.exe")
            .compile()
            .expect("compile Windows application icon and version resources");
    }
}
