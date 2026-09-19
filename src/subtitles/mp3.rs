//! In-memory MP3 muxing, including Xing/LAME delay and padding metadata.
use super::audio::{Chunk, RATE};
use anyhow::{Context, Result, ensure};
use ffmpeg::ffi;
use ffmpeg::packet::Mut;
use ffmpeg_next::{self as ffmpeg, ChannelLayout, codec, format, frame};
use std::ptr;

struct Mux(*mut ffi::AVFormatContext);
impl Drop for Mux {
    fn drop(&mut self) {
        unsafe {
            if !(*self.0).pb.is_null() {
                let mut buffer = ptr::null_mut();
                ffi::avio_close_dyn_buf((*self.0).pb, &mut buffer);
                ffi::av_free(buffer.cast());
            }
            ffi::avformat_free_context(self.0);
        }
    }
}
fn check(code: i32) -> Result<()> {
    if code < 0 {
        return Err(ffmpeg::Error::from(code).into());
    }
    Ok(())
}

pub(super) fn encode(chunk: &Chunk) -> Result<Vec<u8>> {
    ffmpeg::init()?;
    let codec = ffmpeg::encoder::find_by_name("libmp3lame")
        .context("MP3 编码器不可用，请安装含 libmp3lame 的 FFmpeg 或在设置中选择 WAV")?;
    let mut encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .audio()?;
    encoder.set_rate(RATE as i32);
    encoder.set_channel_layout(ChannelLayout::MONO);
    encoder.set_format(format::Sample::F32(format::sample::Type::Planar));
    encoder.set_bit_rate(64_000);
    encoder.set_time_base((1, RATE as i32));
    let mut encoder = encoder.open_as(codec)?;
    let size = encoder.frame_size() as usize;
    ensure!(
        size > 0 && !chunk.samples.is_empty(),
        "empty MP3 audio frame"
    );
    unsafe {
        let mut context = ptr::null_mut();
        check(ffi::avformat_alloc_output_context2(
            &mut context,
            ptr::null_mut(),
            c"mp3".as_ptr(),
            ptr::null(),
        ))?;
        ensure!(!context.is_null(), "MP3 mux allocation failed");
        let mux = Mux(context);
        check(ffi::avio_open_dyn_buf(&mut (*mux.0).pb))?;
        let stream = ffi::avformat_new_stream(mux.0, ptr::null());
        ensure!(!stream.is_null(), "MP3 stream allocation failed");
        (*stream).time_base = ffi::AVRational {
            num: 1,
            den: RATE as i32,
        };
        check(ffi::avcodec_parameters_from_context(
            (*stream).codecpar,
            encoder.as_ptr(),
        ))?;
        check(ffi::avformat_write_header(mux.0, ptr::null_mut()))?;
        let drain = |encoder: &mut ffmpeg::encoder::Audio| -> Result<()> {
            loop {
                let mut packet = ffmpeg::Packet::empty();
                match encoder.receive_packet(&mut packet) {
                    Ok(()) => {
                        packet.set_stream(0);
                        packet.rescale_ts((1, RATE as i32), (*stream).time_base);
                        check(ffi::av_interleaved_write_frame(mux.0, packet.as_mut_ptr()))?;
                    }
                    Err(ffmpeg::Error::Eof) => return Ok(()),
                    Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {
                        return Ok(());
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        };
        for (index, samples) in chunk.samples.chunks(size).enumerate() {
            let mut frame = frame::Audio::new(encoder.format(), samples.len(), ChannelLayout::MONO);
            frame.set_rate(RATE);
            frame.set_pts(Some((index * size) as i64));
            frame.plane_mut::<f32>(0).copy_from_slice(samples);
            encoder.send_frame(&frame)?;
            drain(&mut encoder)?;
        }
        encoder.send_eof()?;
        drain(&mut encoder)?;
        check(ffi::av_write_trailer(mux.0))?;
        let mut buffer = ptr::null_mut();
        let length = ffi::avio_close_dyn_buf((*mux.0).pb, &mut buffer);
        (*mux.0).pb = ptr::null_mut();
        let output = if length > 0 && !buffer.is_null() {
            std::slice::from_raw_parts(buffer, length as usize).to_vec()
        } else {
            Vec::new()
        };
        ffi::av_free(buffer.cast());
        check(length)?;
        ensure!(!output.is_empty(), "MP3 encoding produced no data");
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mp3_roundtrip_preserves_sample_count_and_reduces_payload() {
        let samples: Vec<_> = (0..RATE * 3 + 123)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin() * 0.3)
            .collect();
        let chunk = Chunk {
            start: 0.0,
            samples,
        };
        let bytes = encode(&chunk).unwrap();
        assert!(bytes.len() < chunk.wav().len() / 2);
        let path = std::env::temp_dir().join(format!("replayer-mp3-{}.mp3", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let mut input = super::super::audio::AudioChunks::open(
            &path,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            5,
        )
        .unwrap();
        let mut count = 0;
        while let Some(decoded) = input.next().unwrap() {
            count += decoded.samples.len();
        }
        // Container start_time is rounded to microseconds by AudioChunks;
        // range clipping may round that difference up to one sample.
        assert!(count.abs_diff(chunk.samples.len()) <= 1);
        drop(input);
        std::fs::remove_file(path).unwrap();
    }
}
