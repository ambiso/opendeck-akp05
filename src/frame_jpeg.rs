use image::RgbImage;
use turbojpeg::{Compressor, Image, PixelFormat, Subsamp};

pub(crate) struct FrameEncoder {
    codec: Compressor,
    output: Vec<u8>,
}

impl FrameEncoder {
    pub(crate) fn new() -> turbojpeg::Result<Self> {
        let mut codec = Compressor::new()?;
        codec.set_quality(90)?;
        codec.set_subsamp(Subsamp::None)?;
        Ok(Self {
            codec,
            output: Vec::new(),
        })
    }

    pub(crate) fn encode(&mut self, img: &RgbImage) -> turbojpeg::Result<&[u8]> {
        let width = img.width() as usize;
        let height = img.height() as usize;
        let rgb = Image {
            pixels: img.as_raw().as_slice(),
            width,
            height,
            pitch: 3 * width,
            format: PixelFormat::RGB,
        };
        self.output.resize(self.codec.buf_len(width, height)?, 0);
        let used = self.codec.compress_to_slice(rgb, &mut self.output)?;
        Ok(&self.output[..used])
    }
}

/// Exercise the shipped codec without opening a USB device or an OpenDeck connection.
pub(crate) fn probe() -> Result<(), String> {
    let img = RgbImage::from_fn(800, 160, |x, y| {
        image::Rgb([(x / 4) as u8, y as u8, (x / 8 + y / 2) as u8])
    });
    let mut encoder = FrameEncoder::new().map_err(|e| e.to_string())?;
    let jpeg = encoder.encode(&img).map_err(|e| e.to_string())?;
    let decoded = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?
        .into_rgb8();
    if decoded.dimensions() != img.dimensions()
        || decoded
            .as_raw()
            .iter()
            .zip(img.as_raw())
            .any(|(a, b)| a.abs_diff(*b) > 8)
    {
        return Err("N4 JPEG round-trip mismatch".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn full_strip_round_trips_at_native_dimensions() {
        super::probe().unwrap();
    }
}
