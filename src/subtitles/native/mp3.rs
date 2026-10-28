use super::audio::Chunk;
use anyhow::Result;
pub(super) fn encode(chunk: &Chunk) -> Result<Vec<u8>> {
    Ok(crate::media_backend::engine()?
        .encode_mp3(&chunk.samples)?
        .bytes()
        .to_vec())
}
