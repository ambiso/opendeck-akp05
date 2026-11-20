use image::{DynamicImage, GenericImageView, Rgb, RgbImage, imageops::FilterType};

pub const MAT: Rgb<u8> = Rgb([19, 28, 38]);

/// Keep the instrument at its original proportions. Populate the unused LCD
/// rows with the same material used between the four zones. In the outer
/// few rows, lift only dark pixels of the Glass face's old bezel fade.
pub fn full_height(image: &DynamicImage, size: (usize, usize)) -> DynamicImage {
    let (width, height) = (size.0 as u32, size.1 as u32);
    let (w, h) = image.dimensions();
    let scale = (width as f32 / w as f32).min(height as f32 / h as f32);
    let sw = ((w as f32 * scale).round() as u32).clamp(1, width);
    let sh = ((h as f32 * scale).round() as u32).clamp(1, height);
    let fitted = image.resize_exact(sw, sh, FilterType::Triangle).into_rgb8();
    let (left, top) = ((width - sw) / 2, (height - sh) / 2);
    DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
        if x < left || y < top || x >= left + sw || y >= top + sh {
            return MAT;
        }
        let (x, y) = (x - left, y - top);
        let mut pixel = *fitted.get_pixel(x, y);
        if y.min(sh - 1 - y) < sh / 8 || x.min(sw - 1 - x) < sw / 32 {
            for (channel, base) in pixel.0.iter_mut().zip(MAT.0) {
                *channel = (*channel).max(base);
            }
        }
        pixel
    }))
}

#[cfg(any(windows, test))]
pub type Frames = [Option<std::sync::Arc<RgbImage>>; 4];

/// Rotate after composing so the four logical columns remain aligned with
/// their knobs on the physically inverted LCD.
#[cfg(test)]
pub fn compose(frames: &Frames, center_offset: i16) -> RgbImage {
    compose_on(frames, center_offset, None)
}

#[cfg(test)]
pub fn compose_on(frames: &Frames, center_offset: i16, artwork: Option<&RgbImage>) -> RgbImage {
    let mut img = RgbImage::new(800, 160);
    compose_into(&mut img, frames, center_offset, artwork);
    img
}

#[cfg(any(windows, test))]
pub fn compose_into(
    img: &mut RgbImage,
    frames: &Frames,
    center_offset: i16,
    artwork: Option<&RgbImage>,
) {
    if let Some(background) = artwork {
        img.as_mut().copy_from_slice(background.as_raw());
    } else {
        for p in img.pixels_mut() {
            *p = MAT;
        }
    }
    let (width, height) = img.dimensions();
    for (slot, tile) in frames.iter().enumerate() {
        let Some(tile) = tile else { continue };
        if tile.width() == 0 {
            continue;
        }
        let top = (height as i32 - tile.height() as i32) / 2 + i32::from(center_offset);
        // The part of a section above or below the physical strip is clipped.
        // Both sides are packed RGB, so the visible scanlines copy as slices.
        let from = top.saturating_neg().max(0) as usize;
        let to = top.max(0) as usize;
        let left = slot * 200 * 3;
        for (dst, src) in img
            .as_mut()
            .chunks_exact_mut(width as usize * 3)
            .skip(to)
            .zip(
                tile.as_raw()
                    .chunks_exact(tile.width() as usize * 3)
                    .skip(from),
            )
        {
            let len = src.len().min(dst.len() - left);
            dst[left..left + len].copy_from_slice(&src[..len]);
        }
    }
    image::imageops::rotate180_in_place(img);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "manual N4 compositor benchmark"]
    fn benchmark_compositor() {
        let frames: Frames = std::array::from_fn(|n| {
            Some(std::sync::Arc::new(RgbImage::from_fn(200, 100, |x, y| {
                Rgb([x as u8, y as u8, n as u8])
            })))
        });
        let band = RgbImage::from_fn(800, 160, |x, y| Rgb([(x % 251) as u8, y as u8, 40]));
        let now = std::time::Instant::now();
        for i in 0..1000 {
            std::hint::black_box(compose_on(&frames, (i % 61) as i16 - 30, Some(&band)));
        }
        println!(
            "1000 N4 compositions: {:.2} ms",
            now.elapsed().as_secs_f64() * 1000.0
        );
        std::fs::create_dir_all("target/previews").unwrap();
        compose_on(&frames, 14, Some(&band))
            .save("target/previews/compositor-reference.bmp")
            .unwrap();
    }

    #[test]
    fn scanline_blit_matches_reference_at_every_supported_alignment() {
        let band = RgbImage::from_fn(800, 160, |x, y| Rgb([x as u8, y as u8, 8]));
        let frames: Frames = std::array::from_fn(|i| {
            (i != 2).then(|| {
                std::sync::Arc::new(RgbImage::from_fn(
                    200,
                    if i % 2 == 0 { 100 } else { 160 },
                    |x, y| Rgb([i as u8, x as u8, y as u8]),
                ))
            })
        });
        let mut actual = RgbImage::new(800, 160);
        for offset in -30..=30 {
            let mut expected = band.clone();
            for (i, tile) in frames.iter().enumerate() {
                if let Some(tile) = tile {
                    image::imageops::replace(
                        &mut expected,
                        tile.as_ref(),
                        i as i64 * 200,
                        (160 - i64::from(tile.height())) / 2 + i64::from(offset),
                    );
                }
            }
            image::imageops::rotate180_in_place(&mut expected);
            compose_into(&mut actual, &frames, offset, Some(&band));
            assert!(actual == expected, "offset {offset}");
        }
    }

    #[test]
    fn four_touching_columns_preserve_order_and_independent_changes() {
        let mut frames: Frames = std::array::from_fn(|i| {
            Some(std::sync::Arc::new(RgbImage::from_pixel(
                200,
                160,
                Rgb([40 + i as u8, 80, 100]),
            )))
        });
        let displayed = image::imageops::rotate180(&compose(&frames, 0));
        for x in 0..800 {
            assert_eq!(displayed.get_pixel(x, 80)[0], 40 + (x / 200) as u8);
        }
        frames[2] = None;
        let cleared = image::imageops::rotate180(&compose(&frames, 0));
        for x in 0..800 {
            let expected = if (400..600).contains(&x) {
                &MAT
            } else {
                displayed.get_pixel(x, 80)
            };
            assert_eq!(cleared.get_pixel(x, 80), expected);
        }
    }
    #[test]
    fn wallpaper_fills_empty_slots_and_the_margins_of_populated_slots() {
        let band = RgbImage::from_fn(800, 160, |x, y| Rgb([(x % 199) as u8, y as u8, 51]));
        let mut frames = Frames::default();
        let tile = RgbImage::from_pixel(200, 100, Rgb([200, 88, 99]));
        frames[1] = Some(std::sync::Arc::new(tile));
        let result = image::imageops::rotate180(&compose_on(&frames, 14, Some(&band)));
        assert_eq!(result.get_pixel(230, 43), band.get_pixel(230, 43));
        assert_eq!(result.get_pixel(230, 44), &Rgb([200, 88, 99]));
        assert_eq!(result.get_pixel(230, 144), band.get_pixel(230, 144));
        assert_eq!(result.get_pixel(765, 125), band.get_pixel(765, 125));
    }

    #[test]
    fn positive_content_offset_moves_the_reading_down_in_viewer_coordinates() {
        let mut frames = Frames::default();
        let mut tile = RgbImage::from_pixel(200, 160, MAT);
        tile.put_pixel(50, 80, Rgb([255, 255, 255]));
        frames[0] = Some(std::sync::Arc::new(tile));
        let displayed = image::imageops::rotate180(&compose(&frames, 14));
        assert_eq!(displayed.get_pixel(50, 94), &Rgb([255, 255, 255]));
        assert_eq!(displayed.get_pixel(50, 80), &MAT);
    }
    #[test]
    fn extends_the_surface_without_stretching_a_gauge_or_cropping_text() {
        let bg = Rgb([19, 28, 38]);
        let src = RgbImage::from_fn(200, 100, |x, y| {
            if (x as i32 - 100).pow(2) + (y as i32 - 50).pow(2) < 400 {
                Rgb([255, 255, 255])
            } else {
                bg
            }
        });
        let dst = full_height(&DynamicImage::ImageRgb8(src), (176, 112)).into_rgb8();
        assert_eq!(dst.dimensions(), (176, 112));
        assert!((0..176).all(|x| dst.get_pixel(x, 0) == &bg && dst.get_pixel(x, 111) == &bg));
        let horizontal = (0..176).filter(|&x| dst.get_pixel(x, 56)[0] > 128).count();
        let vertical = (0..112).filter(|&y| dst.get_pixel(88, y)[0] > 128).count();
        assert!(horizontal.abs_diff(vertical) <= 1);
    }
}
