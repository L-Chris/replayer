use anyhow::{Context, Result};
use ffmpeg::{codec, frame};
use ffmpeg_next as ffmpeg;
use std::collections::VecDeque;

/// Ref-counted FFmpeg planes, preserving pixel format, range, matrix and SAR.
/// No RGB allocation or deep frame clone on the decode thread.
pub enum VideoSurface {
    Software(frame::Video),
}
pub struct VideoFrame {
    pub epoch: u64,
    pub pts: f64,
    pub surface: VideoSurface,
}
impl VideoFrame {
    pub fn image(&self) -> &frame::Video {
        match &self.surface {
            VideoSurface::Software(f) => f,
        }
    }
    pub fn aspect(&self) -> f32 {
        let f = self.image();
        let sar = f.aspect_ratio();
        let ratio = if sar.numerator() > 0 && sar.denominator() > 0 {
            f64::from(sar) as f32
        } else {
            1.0
        };
        f.width() as f32 * ratio / f.height().max(1) as f32
    }
}
pub struct VideoPipeline {
    dec: ffmpeg::decoder::Video,
    tb: f64,
    origin: f64,
    next_pts: f64,
    pub pending: VecDeque<VideoFrame>,
    pub end: f64,
    pub frame_duration: f64,
    pub queue_limit: usize,
    pub hardware: bool,
}
impl VideoPipeline {
    pub fn new(params: codec::Parameters, tb: f64, origin: f64) -> Result<Self> {
        let (dec, hardware) = open_decoder(params)?;
        let duration = dec
            .frame_rate()
            .map(f64::from)
            .filter(|r| *r > 0.0)
            .map_or(1.0 / 30.0, |r| 1.0 / r);
        // Conservative 4 bytes/pixel budget; native YUV420 is usually smaller.
        let bytes = (dec.width() as usize * dec.height() as usize * 4).max(1);
        let queue_limit = (64 * 1024 * 1024 / bytes).clamp(1, 8);
        Ok(Self {
            dec,
            tb,
            origin,
            next_pts: 0.0,
            pending: VecDeque::new(),
            end: 0.0,
            frame_duration: duration,
            queue_limit,
            hardware,
        })
    }
    pub fn reset(&mut self, target: f64) {
        self.dec.flush();
        self.pending.clear();
        self.next_pts = target;
        self.end = target;
    }
    pub fn decode(
        &mut self,
        packet: Option<&ffmpeg::Packet>,
        epoch: u64,
        target: f64,
    ) -> Result<()> {
        let sent = match packet {
            Some(p) => self.dec.send_packet(p),
            None => self.dec.send_eof(),
        };
        if let Err(e) = sent {
            if super::again(e) {
                self.receive(epoch, target)?;
                match packet {
                    Some(p) => self.dec.send_packet(p)?,
                    None => self.dec.send_eof()?,
                }
            } else {
                return Err(e.into());
            }
        }
        self.receive(epoch, target)
    }
    fn receive(&mut self, epoch: u64, target: f64) -> Result<()> {
        loop {
            let mut decoded = frame::Video::empty();
            match self.dec.receive_frame(&mut decoded) {
                Ok(()) => {}
                Err(e) if super::again(e) || e == ffmpeg::Error::Eof => break,
                Err(e) => return Err(e).context("decode video"),
            }
            let pts = decoded
                .timestamp()
                .or(decoded.pts())
                .map_or(self.next_pts, |p| p as f64 * self.tb - self.origin);
            let duration = decoded.packet().duration as f64 * self.tb;
            if duration > 0.0 {
                self.frame_duration = duration;
            }
            let bytes = (decoded.width() as usize * decoded.height() as usize * 4).max(1);
            self.queue_limit = (64 * 1024 * 1024 / bytes).clamp(1, 8);
            self.next_pts = pts + self.frame_duration;
            if pts + 0.000001 < target {
                continue;
            }
            // Hardware decode currently downloads native YUV for portable WGPU upload.
            // Cross-API D3D11/WGPU texture sharing is deliberately not assumed.
            self.hardware = unsafe { !(*decoded.as_ptr()).hw_frames_ctx.is_null() };
            let decoded = if self.hardware {
                let mut host = frame::Video::empty();
                let result = unsafe {
                    ffmpeg::ffi::av_hwframe_transfer_data(host.as_mut_ptr(), decoded.as_ptr(), 0)
                };
                if result < 0 {
                    return Err(ffmpeg::Error::from(result)).context("download hardware frame");
                }
                let result = unsafe {
                    ffmpeg::ffi::av_frame_copy_props(host.as_mut_ptr(), decoded.as_ptr())
                };
                if result < 0 {
                    return Err(ffmpeg::Error::from(result))
                        .context("copy hardware frame metadata");
                }
                host
            } else {
                decoded
            };
            self.end = pts + self.frame_duration;
            self.pending.push_back(VideoFrame {
                epoch,
                pts,
                surface: VideoSurface::Software(decoded),
            });
        }
        Ok(())
    }
}
fn open_decoder(params: codec::Parameters) -> Result<(ffmpeg::decoder::Video, bool)> {
    #[cfg(windows)]
    if std::env::var_os("REPLAYER_SOFTWARE").is_none()
        && let Ok(dec) = open_hardware(params.clone())
    {
        return Ok((dec, true));
    }
    Ok((
        codec::context::Context::from_parameters(params)?
            .decoder()
            .video()?,
        false,
    ))
}
#[cfg(windows)]
fn open_hardware(params: codec::Parameters) -> Result<ffmpeg::decoder::Video> {
    use ffmpeg::ffi::*;
    // FFmpeg owns the device reference once attached to the codec context.
    unsafe extern "C" fn choose_format(
        ctx: *mut AVCodecContext,
        formats: *const AVPixelFormat,
    ) -> AVPixelFormat {
        unsafe {
            let mut p = formats;
            while *p != AVPixelFormat::AV_PIX_FMT_NONE {
                if *p == AVPixelFormat::AV_PIX_FMT_D3D11 {
                    return *p;
                }
                p = p.add(1);
            }
            avcodec_default_get_format(ctx, formats)
        }
    }
    let mut context = codec::context::Context::from_parameters(params)?;
    let codec = ffmpeg::decoder::find(context.id()).context("video decoder unavailable")?;
    unsafe {
        let mut supported = false;
        for i in 0..64 {
            let c = avcodec_get_hw_config(codec.as_ptr(), i);
            if c.is_null() {
                break;
            }
            if (*c).device_type == AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA
                && ((*c).methods & AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32) != 0
            {
                supported = true;
                break;
            }
        }
        anyhow::ensure!(supported, "D3D11VA unavailable for this codec");
        let mut device = std::ptr::null_mut();
        let result = av_hwdevice_ctx_create(
            &mut device,
            AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
        );
        if result < 0 {
            return Err(ffmpeg::Error::from(result).into());
        }
        (*context.as_mut_ptr()).hw_device_ctx = device;
        (*context.as_mut_ptr()).get_format = Some(choose_format);
        // Extra surfaces cover decoder references and the bounded presentation queue.
        (*context.as_mut_ptr()).extra_hw_frames = 12;
    }
    Ok(context.decoder().open_as(codec)?.video()?)
}
