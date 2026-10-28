use remdia::{FrameInfo, MediaBuffer};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pixel {
    YUV420P,
    YUVJ420P,
    NV12,
    P010LE,
    YUV420P10LE,
    YUV420P12LE,
    YUV420P16LE,
    P016LE,
    Rgba,
    Bgra,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Jpeg,
    Mpeg,
}
#[derive(Clone, Copy)]
pub enum Space {
    BT709,
    BT2020NCL,
    BT2020CL,
    Unspecified,
    BT601,
}
pub struct Image {
    buffer: MediaBuffer,
    #[cfg(test)]
    owned: Option<Vec<Vec<u8>>>,
}
impl Image {
    fn from_frame(frame: remdia::Frame) -> Self {
        Self {
            buffer: frame.buffer().clone(),
            #[cfg(test)]
            owned: None,
        }
    }
    pub fn width(&self) -> u32 {
        match self.buffer.header.info {
            FrameInfo::Video { width, .. } => width,
            _ => 0,
        }
    }
    pub fn height(&self) -> u32 {
        match self.buffer.header.info {
            FrameInfo::Video { height, .. } => height,
            _ => 0,
        }
    }
    pub fn format(&self) -> Pixel {
        let FrameInfo::Video { format, .. } = &self.buffer.header.info else {
            unreachable!()
        };
        match format.as_str() {
            "nv12" => Pixel::NV12,
            "p010le" => Pixel::P010LE,
            "p016le" => Pixel::P016LE,
            "yuv420p10le" => Pixel::YUV420P10LE,
            "yuv420p12le" => Pixel::YUV420P12LE,
            "yuv420p16le" => Pixel::YUV420P16LE,
            "yuvj420p" => Pixel::YUVJ420P,
            "yuv420p" => Pixel::YUV420P,
            "bgra" => Pixel::Bgra,
            _ => Pixel::Rgba,
        }
    }
    pub fn stride(&self, i: usize) -> usize {
        match &self.buffer.header.info {
            FrameInfo::Video { planes, .. } => planes[i].stride,
            _ => 0,
        }
    }
    pub fn data(&self, i: usize) -> &[u8] {
        #[cfg(test)]
        if let Some(owned) = &self.owned {
            return &owned[i];
        }
        self.buffer.plane(i)
    }
    pub fn color_space(&self) -> Space {
        match &self.buffer.header.info {
            FrameInfo::Video { color_space, .. } => match color_space.to_lowercase().as_str() {
                "bt709" => Space::BT709,
                "bt2020ncl" | "bt2020" => Space::BT2020NCL,
                "bt2020cl" => Space::BT2020CL,
                "bt601" => Space::BT601,
                _ => Space::Unspecified,
            },
            _ => Space::Unspecified,
        }
    }
    pub fn color_range(&self) -> Range {
        if matches!(&self.buffer.header.info,FrameInfo::Video {color_range,..} if color_range=="full")
        {
            Range::Jpeg
        } else {
            Range::Mpeg
        }
    }
    #[cfg(test)]
    pub fn new(format: Pixel, w: u32, h: u32) -> Self {
        let wide = matches!(
            format,
            Pixel::P010LE
                | Pixel::P016LE
                | Pixel::YUV420P10LE
                | Pixel::YUV420P12LE
                | Pixel::YUV420P16LE
        );
        let nv = matches!(format, Pixel::NV12 | Pixel::P010LE | Pixel::P016LE);
        let b = if wide { 2 } else { 1 };
        let strides = if nv {
            vec![w as usize * b, w.div_ceil(2) as usize * 2 * b]
        } else {
            vec![
                w as usize * b,
                w.div_ceil(2) as usize * b,
                w.div_ceil(2) as usize * b,
            ]
        };
        let planes: Vec<_> = strides
            .iter()
            .enumerate()
            .map(|(i, &stride)| remdia::graph::Plane {
                stride,
                rows: if i == 0 {
                    h as usize
                } else {
                    h.div_ceil(2) as usize
                },
            })
            .collect();
        let owned: Vec<_> = planes.iter().map(|p| vec![0; p.stride * p.rows]).collect();
        let info = FrameInfo::Video {
            pts_us: Some(0),
            duration_us: 0,
            width: w,
            height: h,
            format: format!("{format:?}").to_lowercase(),
            planes,
            sample_aspect_ratio: remdia::Rational { num: 1, den: 1 },
            color_space: "unspecified".into(),
            color_range: "limited".into(),
            color_primaries: String::new(),
            color_transfer: String::new(),
            hardware_decoded: false,
        };
        Self {
            buffer: MediaBuffer::new(
                remdia::BufferHeader {
                    stream_id: 0,
                    generation: 1,
                    info,
                },
                Vec::new(),
            ),
            owned: Some(owned),
        }
    }
    #[cfg(test)]
    pub fn data_mut(&mut self, i: usize) -> &mut [u8] {
        &mut self.owned.as_mut().unwrap()[i]
    }
}
pub struct VideoFrame {
    pub epoch: u64,
    pub pts: f64,
    surface: Image,
}
impl VideoFrame {
    pub fn from_remdia(epoch: u64, pts: f64, frame: remdia::Frame) -> Self {
        Self {
            epoch,
            pts,
            surface: Image::from_frame(frame),
        }
    }
    pub fn image(&self) -> &Image {
        &self.surface
    }
    pub fn aspect(&self) -> f32 {
        let ratio = match &self.surface.buffer.header.info {
            FrameInfo::Video {
                sample_aspect_ratio: r,
                ..
            } if r.num > 0 && r.den > 0 => r.num as f32 / r.den as f32,
            _ => 1.,
        };
        self.surface.width() as f32 * ratio / self.surface.height().max(1) as f32
    }
}
