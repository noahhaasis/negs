//! Writing rendered images to disk, shared by the CLI and the GUI.
//!
//! - TIFF: RGB, uncompressed, 16-bit by default or 8-bit.
//! - PNG: RGB, 8-bit by default or 16-bit.
//! - JPEG: 8-bit RGB, quality 95 by default.
//!
//! Values are clamped to [0, 1] and quantised with round-half-to-even,
//! which matches numpy's `np.round` (the Python reference). If
//! [`ImageMetadata`] is given, it is embedded as EXIF: in an `APP1`
//! segment (JPEG), an `eXIf` chunk (PNG), or IFD0 plus an Exif sub-IFD
//! (TIFF). Every file carries the ICC profile of its colour space
//! ([`crate::icc`]).

use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail, ensure};
use image::ImageEncoder;
use rawler::formats::tiff::{DirectoryWriter, Rational, TiffWriter, Value};
use rawler::tags::{ExifTag, TiffCommonTag};
use rayon::prelude::*;
use spektrafilm_math::colorspace::RgbColourspace;
use spektrafilm_math::image::ImageBuf;

use crate::extension;
use crate::metadata::ImageMetadata;

/// JPEG quality. The f64 export path exists to avoid quality loss;
/// `image`'s default of 75 would defeat the point.
const JPEG_QUALITY: u8 = 95;

/// TIFF's `InterColorProfile` tag.
const TIFF_ICC_PROFILE: u16 = 34675;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SaveOptions {
    pub jpeg_quality: u8,
    pub copy_metadata: bool,
    /// Bits per channel, 8 or 16; `None` is the format's default: 16 for
    /// TIFF, 8 for PNG. JPEG is always 8-bit.
    pub bit_depth: Option<u8>,
    /// The colour space the pixels are encoded in, as colour names it (see
    /// `io.output_color_space`). Only its profile is written: the caller
    /// renders or converts the image into it.
    pub color_space: String,
    /// The pixels are linear (`io.output_cctf_encoding` off), so the
    /// profile gets a linear tone curve.
    #[serde(skip)]
    pub linear: bool,
}
impl Default for SaveOptions {
    fn default() -> Self {
        Self {
            jpeg_quality: JPEG_QUALITY,
            copy_metadata: true,
            bit_depth: None,
            color_space: "sRGB".into(),
            linear: false,
        }
    }
}

/// Save `img` as TIFF, PNG or JPEG, chosen by the extension of `path`,
/// embedding `metadata` as EXIF if given.
pub fn save_image(img: &ImageBuf, path: &Path, metadata: Option<&ImageMetadata>) -> Result<()> {
    save_image_with_options(img, path, metadata, &SaveOptions::default())
}

pub fn save_image_with_options(
    img: &ImageBuf,
    path: &Path,
    metadata: Option<&ImageMetadata>,
    options: &SaveOptions,
) -> Result<()> {
    let (format, depth, space) = options.plan(path)?;
    let icc = crate::icc::profile(space, options.linear);
    let metadata = if options.copy_metadata {
        metadata
    } else {
        None
    };
    match format {
        Format::Tiff => save_tiff(img, path, metadata, depth, icc),
        Format::Png => save_png(img, path, metadata, depth, icc),
        Format::Jpeg(quality) => save_jpeg(img, path, metadata, quality, icc),
    }
}

impl SaveOptions {
    /// Fails if these options can't save to `path`, before anything renders.
    pub fn check(&self, path: &Path) -> Result<()> {
        self.plan(path).map(|_| ())
    }

    fn plan(&self, path: &Path) -> Result<(Format, u8, &'static RgbColourspace)> {
        ensure!(
            (1..=100).contains(&self.jpeg_quality),
            "JPEG quality must be between 1 and 100"
        );
        let space = RgbColourspace::find(&self.color_space)
            .with_context(|| format!("unknown colour space: {}", self.color_space))?;
        let ext = extension(path);
        let (format, default_depth) = match ext.as_str() {
            "tif" | "tiff" => (Format::Tiff, 16),
            "png" => (Format::Png, 8),
            "jpg" | "jpeg" => (Format::Jpeg(self.jpeg_quality), 8),
            _ => bail!("unsupported output format: .{ext} (supported: tiff, png, jpg)"),
        };
        let depth = self.bit_depth.unwrap_or(default_depth);
        ensure!(depth == 8 || depth == 16, "bit depth must be 8 or 16, not {depth}");
        ensure!(
            depth == 8 || !matches!(format, Format::Jpeg(_)),
            "JPEG is 8-bit only; save 16 bits as PNG or TIFF"
        );
        Ok((format, depth, space))
    }
}

enum Format {
    Tiff,
    Png,
    Jpeg(u8),
}

fn quantize_u8(img: &ImageBuf) -> Vec<u8> {
    img.data
        .par_iter()
        .map(|&v| (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8)
        .collect()
}

fn quantize_u16(img: &ImageBuf) -> Vec<u16> {
    img.data
        .par_iter()
        .map(|&v| (v.clamp(0.0, 1.0) * 65535.0).round_ties_even() as u16)
        .collect()
}

fn create(path: &Path) -> Result<BufWriter<File>> {
    Ok(BufWriter::new(
        File::create(path).with_context(|| format!("creating {}", path.display()))?,
    ))
}

/// EXIF block for PNG/JPEG. Written even without source metadata, so
/// the file at least records the software that made it.
fn exif_blob(img: &ImageBuf, metadata: Option<&ImageMetadata>) -> Result<Vec<u8>> {
    metadata
        .cloned()
        .unwrap_or_default()
        .to_exif_blob(img.width, img.height)
}

fn save_png(
    img: &ImageBuf,
    path: &Path,
    metadata: Option<&ImageMetadata>,
    depth: u8,
    icc: Vec<u8>,
) -> Result<()> {
    // The encoder takes 16-bit samples in native byte order.
    let (data, color) = if depth == 16 {
        let data = quantize_u16(img).iter().flat_map(|v| v.to_ne_bytes()).collect();
        (data, image::ExtendedColorType::Rgb16)
    } else {
        (quantize_u8(img), image::ExtendedColorType::Rgb8)
    };
    let mut file = create(path)?;
    let mut enc = image::codecs::png::PngEncoder::new(&mut file);
    enc.set_exif_metadata(exif_blob(img, metadata)?)
        .map_err(|e| anyhow!("PNG EXIF: {e}"))?;
    enc.set_icc_profile(icc)
        .map_err(|e| anyhow!("PNG ICC profile: {e}"))?;
    enc.write_image(&data, img.width, img.height, color)
        .with_context(|| format!("saving PNG: {}", path.display()))?;
    file.flush()
        .with_context(|| format!("saving PNG: {}", path.display()))?;
    Ok(())
}

fn save_jpeg(
    img: &ImageBuf,
    path: &Path,
    metadata: Option<&ImageMetadata>,
    quality: u8,
    icc: Vec<u8>,
) -> Result<()> {
    let data = quantize_u8(img);
    let mut file = create(path)?;
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, quality);
    enc.set_exif_metadata(exif_blob(img, metadata)?)
        .map_err(|e| anyhow!("JPEG EXIF: {e}"))?;
    enc.set_icc_profile(icc)
        .map_err(|e| anyhow!("JPEG ICC profile: {e}"))?;
    enc.write_image(&data, img.width, img.height, image::ExtendedColorType::Rgb8)
        .with_context(|| format!("saving JPEG: {}", path.display()))?;
    file.flush()
        .with_context(|| format!("saving JPEG: {}", path.display()))?;
    Ok(())
}

/// Baseline RGB TIFF, uncompressed, in strips. Written with rawler's TIFF
/// writer rather than the `image` crate's, which can't add an Exif sub-IFD.
fn save_tiff(
    img: &ImageBuf,
    path: &Path,
    metadata: Option<&ImageMetadata>,
    depth: u8,
    icc: Vec<u8>,
) -> Result<()> {
    let mut file = create(path)?;
    let tiff = TiffWriter::new(&mut file).map_err(tiff_err)?;
    let written = if depth == 16 {
        write_tiff_image(tiff, &quantize_u16(img), img.width, img.height, metadata, icc)
    } else {
        write_tiff_image(tiff, &quantize_u8(img), img.width, img.height, metadata, icc)
    };
    written
        .and_then(|()| Ok(file.flush()?))
        .with_context(|| format!("saving TIFF: {}", path.display()))?;
    Ok(())
}

/// A TIFF sample, in native byte order: TiffWriter writes the header in it.
trait Sample: Copy {
    const BYTES: usize;
    fn push(self, out: &mut Vec<u8>);
}
impl Sample for u8 {
    const BYTES: usize = 1;
    fn push(self, out: &mut Vec<u8>) {
        out.push(self);
    }
}
impl Sample for u16 {
    const BYTES: usize = 2;
    fn push(self, out: &mut Vec<u8>) {
        out.extend(self.to_ne_bytes());
    }
}

fn write_tiff_image<W: Write + Seek, S: Sample>(
    mut tiff: TiffWriter<W>,
    data: &[S],
    width: u32,
    height: u32,
    metadata: Option<&ImageMetadata>,
    icc: Vec<u8>,
) -> Result<()> {
    let row_len = width as usize * 3;
    if row_len == 0 || height == 0 {
        bail!("empty image ({width}x{height})");
    }
    // ~256 KiB strips: small enough for streaming readers, few enough
    // offsets for big images.
    let rows_per_strip = (256 * 1024 / (row_len * S::BYTES)).clamp(1, height as usize);
    let mut offsets = Vec::new();
    let mut counts = Vec::new();
    let mut bytes = Vec::with_capacity(rows_per_strip * row_len * S::BYTES);
    for strip in data.chunks(rows_per_strip * row_len) {
        bytes.clear();
        strip.iter().for_each(|v| v.push(&mut bytes));
        offsets.push(tiff.write_data(&bytes).map_err(tiff_err)?);
        counts.push(u32::try_from(bytes.len())?);
    }

    let mut root = DirectoryWriter::new();
    root.add_tag(TiffCommonTag::ImageWidth, width);
    root.add_tag(TiffCommonTag::ImageLength, height);
    root.add_tag(TiffCommonTag::BitsPerSample, [8 * S::BYTES as u16; 3]);
    root.add_tag(TiffCommonTag::Compression, 1u16);
    root.add_tag(TiffCommonTag::PhotometricInt, 2u16);
    root.add_tag(TiffCommonTag::StripOffsets, &offsets);
    root.add_tag(TiffCommonTag::SamplesPerPixel, 3u16);
    root.add_tag(TiffCommonTag::RowsPerStrip, rows_per_strip as u32);
    root.add_tag(TiffCommonTag::StripByteCounts, &counts);
    root.add_tag(ExifTag::PlanarConfiguration, 1u16);
    root.add_tag(TiffCommonTag::XResolution, Rational::new(72, 1));
    root.add_tag(TiffCommonTag::YResolution, Rational::new(72, 1));
    root.add_tag(TiffCommonTag::ResolutionUnit, 2u16);
    root.add_untyped_tag(TIFF_ICC_PROFILE, Value::Undefined(icc));
    metadata
        .cloned()
        .unwrap_or_default()
        .write_tags(&mut tiff, &mut root, width, height)?;
    tiff.build(root).map_err(tiff_err)?;
    Ok(())
}

fn tiff_err(e: rawler::formats::tiff::TiffError) -> anyhow::Error {
    anyhow!("{e}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{SOFTWARE, read_metadata};
    use rawler::exif::{Exif, ExifGPS};
    use rawler::formats::tiff::{IFD, SRational, Value};
    use spektrafilm_math::precision::{Scalar, from_f32, to_f32};
    use std::path::PathBuf;

    /// Fresh scratch directory per test, removed on drop.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("spektrafilm-io-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self, file: &str) -> PathBuf {
            self.0.join(file)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sample_metadata() -> ImageMetadata {
        ImageMetadata {
            make: Some("NIKON CORPORATION".into()),
            model: Some("NIKON D750".into()),
            exif: Exif {
                // Both get rewritten on export.
                orientation: Some(6),
                color_space: Some(2),
                lens_make: Some("Nikon".into()),
                lens_model: Some("AF-S NIKKOR 50mm f/1.8G".into()),
                lens_spec: Some([
                    Rational::new(50, 1),
                    Rational::new(50, 1),
                    Rational::new(18, 10),
                    Rational::new(18, 10),
                ]),
                lens_serial_number: Some("123456".into()),
                serial_number: Some("6000001".into()),
                fnumber: Some(Rational::new(28, 10)),
                exposure_time: Some(Rational::new(1, 250)),
                exposure_bias: Some(SRational::new(-2, 3)),
                iso_speed_ratings: Some(400),
                focal_length: Some(Rational::new(50, 1)),
                exposure_program: Some(3),
                metering_mode: Some(5),
                flash: Some(16),
                date_time_original: Some("2024:05:17 18:42:07".into()),
                create_date: Some("2024:05:17 18:42:07".into()),
                modify_date: Some("2024:05:18 09:00:00".into()),
                offset_time_original: Some("+02:00".into()),
                artist: Some("Jane Doe".into()),
                copyright: Some("(c) Jane Doe".into()),
                gps: Some(ExifGPS {
                    gps_version_id: Some([2, 3, 0, 0]),
                    gps_latitude_ref: Some("N".into()),
                    gps_latitude: Some([
                        Rational::new(52, 1),
                        Rational::new(31, 1),
                        Rational::new(1234, 100),
                    ]),
                    gps_longitude_ref: Some("E".into()),
                    gps_longitude: Some([
                        Rational::new(13, 1),
                        Rational::new(24, 1),
                        Rational::new(5678, 100),
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            extra: [
                (
                    ExifTag::FocalLengthIn35mmFormat.into(),
                    Value::Short(vec![75]),
                ),
                (ExifTag::Contrast.into(), Value::Short(vec![1])),
            ]
            .into(),
        }
    }

    /// What an exported file must contain for `sample_metadata()`.
    fn expected_after_export() -> ImageMetadata {
        let mut want = sample_metadata();
        want.exif.orientation = Some(1);
        want.exif.color_space = None;
        want
    }

    /// 3x2 gradient with out-of-range values to exercise clamping.
    fn test_image() -> ImageBuf {
        #[rustfmt::skip]
        let v: [f32; 18] = [
            0.0, 0.25, 0.5,   0.75, 1.0, -0.5,   1.5, 0.1, 0.9,
            0.2, 0.4, 0.6,    0.8, 0.33, 0.66,   0.01, 0.99, 0.5,
        ];
        ImageBuf::from_data(3, 2, v.iter().copied().map(from_f32).collect())
    }

    /// IFD0 of an exported file (TIFF) or EXIF block (PNG/JPEG).
    fn exif_ifd0(path: &Path) -> IFD {
        let blob = match extension(path).as_str() {
            "tif" | "tiff" => std::fs::read(path).unwrap(),
            _ => {
                let mut dec = image::ImageReader::open(path)
                    .unwrap()
                    .with_guessed_format()
                    .unwrap()
                    .into_decoder()
                    .unwrap();
                image::ImageDecoder::exif_metadata(&mut dec)
                    .unwrap()
                    .expect("no EXIF block")
            }
        };
        let sub = [TiffCommonTag::ExifIFDPointer.into()];
        IFD::new_root_with_correction(&mut std::io::Cursor::new(blob), 0, 0, 0, 1, &sub).unwrap()
    }

    fn roundtrip(ext: &str) {
        let dir = Scratch::new(&format!("roundtrip-{ext}"));
        let out = dir.path(&format!("out.{ext}"));
        let img = test_image();
        save_image(&img, &out, Some(&sample_metadata())).unwrap();

        assert_eq!(read_metadata(&out), Some(expected_after_export()), "{ext}");

        let ifd0 = exif_ifd0(&out);
        let software = ifd0.get_entry(ExifTag::Software).unwrap();
        assert_eq!(software.value.as_string().unwrap(), SOFTWARE);
        let exif = ifd0.get_sub_ifd(ExifTag::ExifOffset).expect("Exif IFD");
        let dim = |tag| exif.get_entry(tag).unwrap().value.force_u32(0);
        assert_eq!(dim(ExifTag::ExifImageWidth), 3);
        assert_eq!(dim(ExifTag::ExifImageHeight), 2);

        // The file is still a valid image the `image` crate decodes, at the
        // right size, and with Orientation 1 it is shown as-is.
        let mut dec = image::ImageReader::open(&out)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .into_decoder()
            .unwrap();
        assert_eq!(
            image::ImageDecoder::orientation(&mut dec).unwrap(),
            image::metadata::Orientation::NoTransforms
        );
        let back = image::DynamicImage::from_decoder(dec).unwrap();
        assert_eq!((back.width(), back.height()), (3, 2));
        match ext {
            "png" => assert_eq!(back.to_rgb8().into_raw(), quantize_u8(&img)),
            "tif" => assert_eq!(back.to_rgb16().into_raw(), quantize_u16(&img)),
            _ => {} // JPEG is lossy.
        }
    }

    #[test]
    fn png_roundtrip_keeps_metadata() {
        roundtrip("png");
    }

    #[test]
    fn jpeg_roundtrip_keeps_metadata() {
        roundtrip("jpg");
    }

    #[test]
    fn tiff_roundtrip_keeps_metadata() {
        roundtrip("tif");
    }

    /// Source → export → re-export: metadata survives a second hop, e.g.
    /// when a saved JPEG/TIFF is loaded again and re-rendered.
    #[test]
    fn metadata_survives_chained_exports() {
        let dir = Scratch::new("chain");
        let img = test_image();
        let mut prev = sample_metadata();
        for name in ["a.jpg", "b.tif", "c.png", "d.jpeg", "e.tiff"] {
            let out = dir.path(name);
            save_image(&img, &out, Some(&prev)).unwrap();
            prev = read_metadata(&out).unwrap_or_else(|| panic!("{name}: lost metadata"));
            assert_eq!(prev, expected_after_export(), "{name}");
        }
    }

    #[test]
    fn exported_tiff_loads_through_load_image() {
        let dir = Scratch::new("tiff-load");
        let out = dir.path("out.tiff");
        let img = test_image();
        save_image(&img, &out, Some(&sample_metadata())).unwrap();
        let back = crate::load_image(&out, crate::Formats::TiffPngRaw).unwrap();
        assert_eq!((back.width, back.height), (3, 2));
        // The export embeds an sRGB profile, so the loader decodes the
        // stored codes back to linear light through the profile's
        // 1024-entry u16 curve table: interpolation and table rounding
        // stay below 5e-5.
        let want: Vec<Scalar> = quantize_u16(&img)
            .into_iter()
            .map(|v| spektrafilm_math::precision::srgb_decode(from_f32(v as f32 / 65535.0)))
            .collect();
        for (got, want) in back.data.iter().zip(&want) {
            assert!((to_f32(*got) - to_f32(*want)).abs() < 5e-5, "{got:?} vs {want:?}");
        }
    }

    /// Big enough for several TIFF strips, and non-square, to catch
    /// strip offset and width/height mixups.
    #[test]
    fn multi_strip_tiff_roundtrip() {
        let dir = Scratch::new("tiff-strips");
        let out = dir.path("big.tif");
        let (w, h) = (301u32, 457u32);
        let data: Vec<Scalar> = (0..w * h * 3)
            .map(|i| from_f32((i % 65536) as f32 / 65535.0))
            .collect();
        let img = ImageBuf::from_data(w, h, data);
        save_image(&img, &out, None).unwrap();
        let back = image::open(&out).unwrap();
        assert_eq!((back.width(), back.height()), (w, h));
        assert_eq!(back.to_rgb16().into_raw(), quantize_u16(&img));
        let ifd0 = exif_ifd0(&out);
        assert!(
            ifd0.get_entry(TiffCommonTag::StripOffsets)
                .unwrap()
                .value
                .count()
                > 1
        );
    }

    #[test]
    fn save_options_keep_defaults_and_control_quality_and_metadata() {
        let dir = Scratch::new("save-options");
        let original = dir.path("default.jpg");
        let explicit = dir.path("explicit.jpg");
        let low = dir.path("quality35.jpg");
        let image = test_image();
        let metadata = sample_metadata();
        save_image(&image, &original, Some(&metadata)).unwrap();
        save_image_with_options(&image, &explicit, Some(&metadata), &SaveOptions::default())
            .unwrap();
        assert_eq!(
            std::fs::read(&original).unwrap(),
            std::fs::read(&explicit).unwrap()
        );
        save_image_with_options(
            &image,
            &low,
            Some(&metadata),
            &SaveOptions {
                jpeg_quality: 35,
                copy_metadata: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_ne!(
            std::fs::read(&original).unwrap(),
            std::fs::read(&low).unwrap()
        );
        assert_eq!(read_metadata(&low), None);
        assert!(read_metadata(&original).is_some());
        let invalid = dir.path("invalid.jpg");
        assert!(
            save_image_with_options(
                &image,
                &invalid,
                None,
                &SaveOptions {
                    jpeg_quality: 0,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(!invalid.exists());
    }
    #[test]
    fn bit_depths_and_colour_profiles() {
        let dir = Scratch::new("depth-icc");
        let img = test_image();
        let p3 = SaveOptions { color_space: "Display P3".into(), ..Default::default() };
        let cases = [
            ("default.png", SaveOptions::default(), 8, "sRGB"),
            ("deep.png", SaveOptions { bit_depth: Some(16), ..p3.clone() }, 16, "Display P3"),
            ("default.tif", p3.clone(), 16, "Display P3"),
            ("shallow.tif", SaveOptions { bit_depth: Some(8), ..Default::default() }, 8, "sRGB"),
            ("photo.jpg", p3.clone(), 8, "Display P3"),
        ];
        for (name, options, depth, space) in cases {
            let out = dir.path(name);
            save_image_with_options(&img, &out, None, &options).unwrap();
            let mut dec = image::ImageReader::open(&out)
                .unwrap()
                .with_guessed_format()
                .unwrap()
                .into_decoder()
                .unwrap();
            // `image` misses a small TIFF's profile; libtiff reads it, as rawler does.
            let icc = if name.ends_with(".tif") {
                match &exif_ifd0(&out).entries().get(&TIFF_ICC_PROFILE).unwrap().value {
                    Value::Undefined(icc) => Some(icc.clone()),
                    other => panic!("{name}: {other:?}"),
                }
            } else {
                image::ImageDecoder::icc_profile(&mut dec).unwrap()
            };
            let space = RgbColourspace::find(space).unwrap();
            assert_eq!(icc, Some(crate::icc::profile(space, false)), "{name}");
            let back = image::DynamicImage::from_decoder(dec).unwrap();
            assert_eq!(back.color().bits_per_pixel() / 3, depth, "{name}");
            if depth == 16 {
                assert_eq!(back.to_rgb16().into_raw(), quantize_u16(&img), "{name}");
            } else if !name.ends_with(".jpg") {
                assert_eq!(back.to_rgb8().into_raw(), quantize_u8(&img), "{name}");
            }
        }
        let jpeg16 = SaveOptions { bit_depth: Some(16), ..Default::default() };
        assert!(jpeg16.check(Path::new("x.jpg")).is_err());
        assert!(SaveOptions { bit_depth: Some(12), ..Default::default() }.check(Path::new("x.png")).is_err());
        let unknown = SaveOptions { color_space: "Nope".into(), ..Default::default() };
        assert!(unknown.check(Path::new("x.png")).is_err());
    }

    #[test]
    fn without_metadata_only_software_is_written() {
        let dir = Scratch::new("no-meta");
        for name in ["a.png", "b.jpg", "c.tif"] {
            let out = dir.path(name);
            save_image(&test_image(), &out, None).unwrap();
            assert_eq!(read_metadata(&out), None, "{name}");
            let ifd0 = exif_ifd0(&out);
            let software = ifd0.get_entry(ExifTag::Software).unwrap();
            assert_eq!(software.value.as_string().unwrap(), SOFTWARE, "{name}");
            assert!(ifd0.get_entry(ExifTag::Make).is_none(), "{name}");
        }
    }

    #[test]
    fn exif_dimensions_follow_the_exported_image() {
        // After a 90° turn the exported image is 2x3; the EXIF must say so.
        let blob = sample_metadata().to_exif_blob(2, 3).unwrap();
        let sub = [TiffCommonTag::ExifIFDPointer.into()];
        let ifd0 = IFD::new_root_with_correction(&mut std::io::Cursor::new(blob), 0, 0, 0, 1, &sub)
            .unwrap();
        let exif = ifd0.get_sub_ifd(ExifTag::ExifOffset).unwrap();
        let dim = |tag| exif.get_entry(tag).unwrap().value.force_u32(0);
        assert_eq!(
            (dim(ExifTag::ExifImageWidth), dim(ExifTag::ExifImageHeight)),
            (2, 3)
        );
    }

    #[test]
    fn quantize_rounds_half_to_even_and_clamps() {
        // 0.5/255 * 255 = 0.5 → 0; 1.5/255 * 255 = 1.5 → 2 (numpy semantics).
        let img = ImageBuf::from_data(
            1,
            2,
            [0.5 / 255.0, 1.5 / 255.0, -1.0, 2.0, 1.0, 0.0]
                .iter()
                .map(|&v: &f64| v as Scalar)
                .collect(),
        );
        assert_eq!(quantize_u8(&img), vec![0, 2, 0, 255, 255, 0]);
    }

    #[test]
    fn rejects_unknown_extension() {
        let err = save_image(&test_image(), Path::new("x.bmp"), None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "unsupported output format: .bmp (supported: tiff, png, jpg)"
        );
    }

    #[test]
    fn read_metadata_is_none_for_missing_or_foreign_files() {
        let dir = Scratch::new("foreign");
        assert_eq!(read_metadata(&dir.path("missing.jpg")), None);
        assert_eq!(read_metadata(&dir.path("missing.nef")), None);
        let junk = dir.path("junk.tif");
        std::fs::write(&junk, b"not a tiff at all").unwrap();
        assert_eq!(read_metadata(&junk), None);
        // A plain PNG without an eXIf chunk.
        let plain = dir.path("plain.png");
        image::RgbImage::new(2, 2).save(&plain).unwrap();
        assert_eq!(read_metadata(&plain), None);
    }

    #[test]
    fn empty_strings_are_not_written() {
        let mut meta = sample_metadata();
        meta.exif.lens_serial_number = Some(String::new());
        meta.exif.artist = Some("  ".into());
        let dir = Scratch::new("empty-strings");
        let out = dir.path("out.jpg");
        save_image(&test_image(), &out, Some(&meta)).unwrap();
        let ifd0 = exif_ifd0(&out);
        assert!(ifd0.get_entry(ExifTag::Artist).is_none());
        let exif = ifd0.get_sub_ifd(ExifTag::ExifOffset).unwrap();
        assert!(exif.get_entry(ExifTag::LensSerialNumber).is_none());
        assert!(exif.get_entry(ExifTag::LensModel).is_some());
    }

    /// Fuji RAF isn't TIFF-based; its EXIF sits in the embedded preview
    /// JPEG. Simulate a non-TIFF container with an `Exif\0\0` block inside.
    #[test]
    fn raw_container_finds_embedded_exif_block() {
        let blob = sample_metadata().to_exif_blob(3, 2).unwrap();
        let mut file = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
        file.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE1, 0, 0]);
        file.extend_from_slice(b"Exif\0\0");
        file.extend_from_slice(&blob);
        file.extend_from_slice(&[0u8; 64]);
        let got = crate::metadata::raw_container_metadata(&file).unwrap();
        assert_eq!(got.make.as_deref(), Some("NIKON CORPORATION"));
        assert_eq!(got.model.as_deref(), Some("NIKON D750"));
        assert_eq!(got.extra, sample_metadata().extra);
        assert_eq!(
            crate::metadata::raw_container_metadata(b"no exif here"),
            None
        );
    }
}
