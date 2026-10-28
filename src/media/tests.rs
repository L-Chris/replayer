use super::*;
#[test]
#[ignore = "requires staged remdia components and REPLAYER_NATIVE_FIXTURES"]
fn native_media_video_thumbnail_and_embedded_cover() {
    let root = PathBuf::from(std::env::var("REPLAYER_NATIVE_FIXTURES").unwrap());
    for source in ["av.mp4", "av.mkv"] {
        let info = probe_info(root.join(source).to_str().unwrap()).expect("probe video");
        assert_eq!(info.kind, Kind::Video);
        let art = info.artwork.as_ref().expect("video thumbnail");
        assert_eq!((art.width, art.height), (160, 90));
        assert_eq!(art.rgba.len(), art.width * art.height * 4);
        assert!(
            art.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] > 100 || p[1] > 100 || p[2] > 100)
        );
    }
    let info = probe_info(root.join("cover.flac").to_str().unwrap()).expect("probe audio");
    assert_eq!(info.kind, Kind::Music);
    assert!(info.artwork.is_some());
}
#[test]
#[ignore = "Reads REPLAYER_TEST_VIDEO and saves its thumbnail for visual inspection"]
fn live_video_thumbnail() {
    let path = std::env::var("REPLAYER_TEST_VIDEO").expect("set REPLAYER_TEST_VIDEO");
    let info = probe_info(&path).expect("probe media");
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
}
