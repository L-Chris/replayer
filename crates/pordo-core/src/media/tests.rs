use super::*;
#[cfg(test)]
use crate::pixels::test_support::TestImageOutput;
#[test]
#[ignore = "requires staged remdia components and PORDO_NATIVE_FIXTURES"]
fn native_media_video_thumbnail_and_embedded_cover() {
    let root = PathBuf::from(crate::environment::var("PORDO_NATIVE_FIXTURES").unwrap());
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
#[ignore = "Reads PORDO_TEST_VIDEO and saves its thumbnail for visual inspection"]
fn live_video_thumbnail() {
    let path = crate::environment::var("PORDO_TEST_VIDEO").expect("set PORDO_TEST_VIDEO");
    let info = probe_info(&path).expect("probe media");
    let art = info.artwork.as_ref().expect("video thumbnail");
    std::fs::create_dir_all("target/ui-review").unwrap();
    crate::pixels::Image::from_rgba(art.width as u32, art.height as u32, art.rgba.clone())
        .unwrap()
        .save("target/ui-review/video-thumbnail.png")
        .unwrap();
}

#[test]
fn old_download_companions_remain_readable() {
    let directory = std::env::temp_dir().join(format!("pordo-sidecar-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let audio = directory.join("song.flac");
    let old_metadata = companion_path(&audio, ".replayer.json");
    std::fs::write(
        &old_metadata,
        br#"{"title":"Existing download","artist":"Artist"}"#,
    )
    .unwrap();
    let old_cover = companion_path(&audio, ".replayer-cover.png");
    std::fs::write(&old_cover, b"existing cover").unwrap();
    assert_eq!(LocalMetadata::load(&audio).title, "Existing download");
    assert_eq!(companion_for_read(&audio, "-cover.png"), old_cover);
    LocalMetadata {
        title: "Updated title".into(),
        ..Default::default()
    }
    .save(&audio)
    .unwrap();
    assert_eq!(LocalMetadata::load(&audio).title, "Updated title");
    assert!(old_metadata.exists());
    std::fs::remove_dir_all(directory).unwrap();
}
