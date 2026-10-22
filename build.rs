use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
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

    let Some(ffmpeg_dir) = env::var_os("FFMPEG_DIR") else {
        return;
    };
    let bin = Path::new(&ffmpeg_dir).join("bin");
    if !bin.is_dir() {
        return;
    }

    let out_dir = env::var("OUT_DIR").unwrap();
    let profile_dir = Path::new(&out_dir).join("../../..");

    for name in [
        "avcodec-63.dll",
        "avformat-63.dll",
        "avutil-61.dll",
        "swresample-7.dll",
        "swscale-10.dll",
    ] {
        // A running preview may lock an existing DLL during cargo check/test.
        // Packaging validates the required runtime files separately.
        let _ = fs::copy(bin.join(name), profile_dir.join(name));
    }
}
