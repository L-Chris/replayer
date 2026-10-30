//! SHA-256 formatting shared by cache keys, downloads and integrity checks.
use aws_lc_rs::digest;

pub fn sha256_hex(bytes: &[u8]) -> String {
    digest_hex(digest::digest(&digest::SHA256, bytes))
}

pub fn digest_hex(value: digest::Digest) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = value.as_ref();
    let mut text = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 15) as usize] as char);
    }
    text
}
