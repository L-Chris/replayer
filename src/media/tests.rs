use super::*;

#[test]
fn video_thumbnails_handle_short_clips_black_leaders_and_portrait_aspect() {
    let root = std::env::temp_dir().join(format!("replayer-thumbnails-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let ffmpeg = crate::test_config::ffmpeg();
    for (name, source, ratio) in [
        (
            "single-frame.mp4",
            "color=c=blue:s=160x90:r=25:d=0.04",
            160.0 / 90.0,
        ),
        (
            "black-leader.mp4",
            "color=c=black:s=160x90:r=25:d=1[a];color=c=blue:s=160x90:r=25:d=1[b];[a][b]concat=n=2:v=1:a=0",
            160.0 / 90.0,
        ),
        (
            "portrait.mp4",
            "color=c=blue:s=90x160:r=25:d=0.8",
            90.0 / 160.0,
        ),
    ] {
        let path = root.join(name);
        assert!(
            std::process::Command::new(&ffmpeg)
                .args([
                    "-v", "error", "-f", "lavfi", "-i", source, "-c:v", "mpeg4", "-bf", "3",
                    "-pix_fmt", "yuv420p"
                ])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let info = probe_info(path.to_str().unwrap()).unwrap();
        assert_eq!(info.kind, Kind::Video);
        let art = info
            .artwork
            .as_ref()
            .unwrap_or_else(|| panic!("missing thumbnail for {name}"));
        assert!(
            (art.width as f64 / art.height as f64 - ratio).abs() < 0.02,
            "distorted thumbnail: {art:?}"
        );
        assert_eq!(art.rgba.len(), art.width * art.height * 4);
        assert!(art.rgba.as_chunks::<4>().0.iter().any(|p| p[2] > 100));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "Reads REPLAYER_TEST_VIDEO and writes its extracted thumbnail for visual inspection"]
fn live_video_thumbnail() {
    let path = std::env::var("REPLAYER_TEST_VIDEO").expect("set REPLAYER_TEST_VIDEO");
    let started = std::time::Instant::now();
    let info = probe_info(&path).expect("probe media");
    assert_eq!(info.kind, Kind::Video);
    let art = info.artwork.as_ref().expect("video thumbnail");
    std::fs::create_dir_all("target/ui-review").unwrap();
    image::save_buffer(
        "target/ui-review/video-thumbnail.png",
        &art.rgba,
        art.width as u32,
        art.height as u32,
        image::ColorType::Rgba8,
    )
    .unwrap();
    println!(
        "Thumbnail: {}x{}, {:.2}s",
        art.width,
        art.height,
        started.elapsed().as_secs_f32()
    );
}
