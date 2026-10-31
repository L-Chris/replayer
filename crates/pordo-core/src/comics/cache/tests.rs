use super::*;
#[cfg(test)]
use crate::pixels::test_support::TestImageOutput;
use std::io::Cursor;

#[test]
fn preview_resize_preserves_alpha_dimensions_and_tile_rows() {
    let mut bytes = Cursor::new(Vec::new());
    let image = std::convert::identity(crate::pixels::generated(8, 8, |x, _| {
        if x < 4 {
            [255, 0, 0, 0]
        } else {
            [0, 0, 255, 255]
        }
    }));
    image.write_to(&mut bytes, "png").unwrap();
    let resized = decode(bytes.get_ref(), 4).unwrap();
    assert_eq!((resized.width, resized.height), (4, 4));
    for pixel in resized.tiles[0].pixels() {
        if pixel[3] > 0 {
            assert_eq!(pixel[0], 0, "transparent RGB leaked into a visible edge");
        }
    }
    let mut bytes = Cursor::new(Vec::new());
    std::convert::identity(crate::pixels::generated(4, 4100, |_, y| {
        [(y % 251) as u8, 0, 0, 255]
    }))
    .write_to(&mut bytes, "png")
    .unwrap();
    let bitmap = decode(bytes.get_ref(), 1800).unwrap();
    assert_eq!(
        bitmap.tiles.iter().map(|t| t.height()).collect::<Vec<_>>(),
        vec![2048, 2048, 4]
    );
    for (i, pixel) in bitmap
        .tiles
        .iter()
        .flat_map(|t| t.bytes().chunks(t.info().stride).map(|row| row[0]))
        .enumerate()
    {
        assert_eq!(pixel, (i % 251) as u8);
    }
}
