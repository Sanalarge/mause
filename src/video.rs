use openh264::OpenH264API;
use openh264::decoder::Decoder;
use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, Profile, RateControlMode,
    UsageType,
};
use openh264::formats::{BgraSliceU8, YUVBuffer, YUVSource};
use std::borrow::Cow;
use std::io;

const MAX_SIDE: usize = 3840;
const MAX_PIXELS: usize = 3840 * 2160;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn check_size(width: usize, height: usize) -> io::Result<()> {
    if width < 2
        || height < 2
        || width > MAX_SIDE
        || height > MAX_SIDE
        || width * height > MAX_PIXELS
        || !width.is_multiple_of(2)
        || !height.is_multiple_of(2)
    {
        return Err(invalid("video boyutu desteklenmiyor"));
    }
    Ok(())
}

pub fn is_h264(payload: &[u8]) -> bool {
    payload.starts_with(&[0, 0, 0, 1]) || payload.starts_with(&[0, 0, 1])
}

pub fn even(width: usize, height: usize, bgra: &[u8]) -> (usize, usize, Cow<'_, [u8]>) {
    let (even_width, even_height) = (width & !1, height & !1);
    if (even_width, even_height) == (width, height) {
        return (width, height, Cow::Borrowed(bgra));
    }
    let mut out = Vec::with_capacity(even_width * even_height * 4);
    for row in 0..even_height {
        let start = row * width * 4;
        out.extend_from_slice(&bgra[start..start + even_width * 4]);
    }
    (even_width, even_height, Cow::Owned(out))
}

pub fn bitrate(width: usize, height: usize, fps: u8, quality: u8) -> u32 {
    ((width * height) as u64 * fps as u64 * (quality as u64 + 20) / 1000).clamp(250_000, 20_000_000)
        as u32
}

pub struct H264Encoder {
    encoder: Encoder,
    yuv: YUVBuffer,
    width: usize,
    height: usize,
}

impl H264Encoder {
    pub fn new(width: usize, height: usize, fps: u8, bitrate: u32) -> io::Result<Self> {
        check_size(width, height)?;
        let config = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .profile(Profile::Baseline)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(bitrate))
            .max_frame_rate(FrameRate::from_hz(fps.max(1) as f32))
            .intra_frame_period(IntraFramePeriod::from_num_frames(fps.max(1) as u32 * 2))
            .num_threads(2)
            .adaptive_quantization(false)
            .background_detection(false)
            .skip_frames(true);
        let encoder = Encoder::with_api_config(OpenH264API::from_source(), config)
            .map_err(io::Error::other)?;
        Ok(Self {
            encoder,
            yuv: YUVBuffer::new(width, height),
            width,
            height,
        })
    }

    pub fn encode(&mut self, bgra: &[u8]) -> io::Result<Vec<u8>> {
        if bgra.len() != self.width * self.height * 4 {
            return Err(invalid("görüntü tamponu boyutu uyuşmuyor"));
        }
        self.yuv
            .read_bgra8(BgraSliceU8::new(bgra, (self.width, self.height)));
        self.encoder
            .encode(&self.yuv)
            .map(|bits| bits.to_vec())
            .map_err(io::Error::other)
    }
}

pub struct H264Decoder {
    decoder: Decoder,
}

impl H264Decoder {
    pub fn new() -> io::Result<Self> {
        Decoder::new()
            .map(|decoder| Self { decoder })
            .map_err(io::Error::other)
    }

    pub fn decode(&mut self, data: &[u8]) -> io::Result<Option<(usize, usize, Vec<u8>)>> {
        let Some(yuv) = self.decoder.decode(data).map_err(io::Error::other)? else {
            return Ok(None);
        };
        let (width, height) = yuv.dimensions();
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(invalid("çözülen video boyutu desteklenmiyor"));
        }
        let mut rgb = vec![0u8; width * height * 3];
        yuv.write_rgb8(&mut rgb);
        Ok(Some((width, height, rgb)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn frame(width: usize, height: usize, shift: usize) -> Vec<u8> {
        let mut bgra = vec![255u8; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let at = (y * width + x) * 4;
                bgra[at] = ((x + shift) * 255 / width) as u8;
                bgra[at + 1] = (y * 255 / height) as u8;
                bgra[at + 2] = 120;
            }
        }
        bgra
    }

    #[test]
    fn kare_dizisi_kodlanip_cozulur() {
        let (width, height) = (640, 360);
        let mut encoder = H264Encoder::new(width, height, 30, 4_000_000).unwrap();
        let mut decoder = H264Decoder::new().unwrap();
        let mut decoded = 0;
        for index in 0..8 {
            let packet = encoder.encode(&frame(width, height, index * 8)).unwrap();
            if packet.is_empty() {
                continue;
            }
            assert!(is_h264(&packet), "Annex-B başlangıç kodu bekleniyordu");
            if let Some((w, h, rgb)) = decoder.decode(&packet).unwrap() {
                assert_eq!((w, h), (width, height));
                let mid = (h / 2 * w + w / 2) * 3;
                assert!(rgb[mid..mid + 3].iter().any(|value| *value > 40));
                decoded += 1;
            }
        }
        assert!(decoded >= 4, "yalnız {decoded} kare çözüldü");
    }

    #[test]
    fn tek_sayili_boyut_ciftlenir_ve_gecersiz_boyut_reddedilir() {
        let source = vec![9u8; 5 * 3 * 4];
        let (width, height, data) = even(5, 3, &source);
        assert_eq!((width, height, data.len()), (4, 2, 4 * 2 * 4));
        assert!(H264Encoder::new(5, 4, 30, 1_000_000).is_err());
        assert!(H264Encoder::new(8000, 4000, 30, 1_000_000).is_err());
        assert!(!is_h264(&[0xFF, 0xD8, 0xFF]));
        assert_eq!(bitrate(1920, 1080, 30, 70), 5_598_720);
    }

    #[test]
    #[ignore = "performans: gerçek işlemci hızına bağlıdır"]
    fn full_hd_kodlama_hizi() {
        let (width, height) = (1920, 1080);
        let mut encoder = H264Encoder::new(width, height, 30, 6_000_000).unwrap();
        let start = Instant::now();
        for index in 0..30 {
            encoder.encode(&frame(width, height, index * 4)).unwrap();
        }
        println!("1080p 30 kare: {:?}", start.elapsed());
    }
}
