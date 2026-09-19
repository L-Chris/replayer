use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");

    let Some(ffmpeg_dir) = env::var_os("FFMPEG_DIR") else {
        return;
    };
    let bin = Path::new(&ffmpeg_dir).join("bin");
    if !bin.is_dir() {
        return;
    }

    let out_dir = env::var("OUT_DIR").unwrap();
    let profile_dir = Path::new(&out_dir).join("../../..");

    let Ok(entries) = fs::read_dir(&bin) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("dll")
            && let Some(name) = path.file_name()
        {
            let _ = fs::copy(&path, profile_dir.join(name));
        }
    }
}
