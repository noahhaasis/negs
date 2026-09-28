//! Camera metadata (EXIF) carried from the input file to the exported one.
//!
//! [`read_metadata`] reads the camera body, lens, exposure settings,
//! capture dates, GPS position and copyright/artist from a camera RAW,
//! JPEG, PNG, WebP or TIFF input. [`ImageMetadata::to_exif_blob`] writes
//! them back out as a TIFF-structured EXIF block, which the PNG, JPEG and
//! TIFF writers in [`crate::save`] embed in the file.
//!
//! The tags rawler knows (its [`Exif`] struct) are carried over, plus a
//! few more from [`EXTRA_EXIF_TAGS`] (35 mm equivalent focal length, ...)
//! when the source has a TIFF-structured EXIF block. Maker notes are
//! dropped: they are vendor-specific and often use absolute file offsets,
//! so they can't be moved into another file.
//!
//! Two tags are rewritten instead of copied:
//! - `Orientation` is always 1: the loader bakes the EXIF rotation (and
//!   the GUI's manual rotation) into the pixels.
//! - `ColorSpace` is dropped: the camera's setting (e.g. Adobe RGB) says
//!   nothing about the colour space of the rendered output.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;

use anyhow::{Result, anyhow};
use rawler::decoders::{RawDecodeParams, RawMetadata};
use rawler::exif::Exif;
use rawler::formats::tiff::{DirectoryWriter, IFD, TiffWriter, Value};
use rawler::tags::{ExifTag, TiffCommonTag};

use crate::{extension, is_raw_extension};

/// Value of the EXIF `Software` tag in exported files.
pub const SOFTWARE: &str = concat!("spektrafilm ", env!("CARGO_PKG_VERSION"));

/// Exif-IFD tags that rawler's [`Exif`] struct doesn't cover but that
/// describe the shot and stay true for the rendered image. Copied
/// verbatim.
pub const EXTRA_EXIF_TAGS: [ExifTag; 6] = [
    ExifTag::FocalLengthIn35mmFormat,
    ExifTag::DigitalZoomRatio,
    ExifTag::Contrast,
    ExifTag::Saturation,
    ExifTag::Sharpness,
    ExifTag::GainControl,
];

/// Metadata of a source image that is written into exported files.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageMetadata {
    /// Camera maker (EXIF `Make`).
    pub make: Option<String>,
    /// Camera model (EXIF `Model`).
    pub model: Option<String>,
    /// Lens, exposure, dates, GPS, ...
    pub exif: Exif,
    /// Values of [`EXTRA_EXIF_TAGS`] found in the source, by tag number.
    pub extra: BTreeMap<u16, Value>,
}

impl ImageMetadata {
    /// True if there is no camera metadata at all. `Orientation` and
    /// `ColorSpace` don't count: they are never copied (see the module
    /// docs), and every exported file has `Orientation = 1`.
    pub fn is_empty(&self) -> bool {
        let mut exif = self.exif.clone();
        exif.orientation = None;
        exif.color_space = None;
        self.make.is_none()
            && self.model.is_none()
            && exif == Exif::default()
            && self.extra.is_empty()
    }

    /// Serialise as a standalone EXIF block (a TIFF header followed by
    /// IFD0, the Exif sub-IFD and, if present, the GPS sub-IFD). This is
    /// the payload of a JPEG `APP1 Exif` segment (without the
    /// `Exif\0\0` prefix) and of a PNG `eXIf` chunk. `width`/`height` are
    /// the dimensions of the exported image. `Software` is set to
    /// [`SOFTWARE`].
    pub fn to_exif_blob(&self, width: u32, height: u32) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        let mut tiff = TiffWriter::new(Cursor::new(&mut buf)).map_err(tiff_err)?;
        let mut root = DirectoryWriter::new();
        self.write_tags(&mut tiff, &mut root, width, height)?;
        tiff.build(root).map_err(tiff_err)?;
        Ok(buf)
    }

    /// Add the metadata to `root` (IFD0) of a TIFF being written with
    /// `tiff`. The Exif and GPS sub-IFDs are written into `tiff` straight
    /// away and linked from `root`.
    pub(crate) fn write_tags<W: Write + Seek>(
        &self,
        tiff: &mut TiffWriter<W>,
        root: &mut DirectoryWriter,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let mut exif = self.exif.clone();
        exif.orientation = Some(1);
        exif.color_space = None;
        drop_empty_strings(&mut exif);
        let raw_meta = RawMetadata {
            exif,
            make: self.make.clone().unwrap_or_default(),
            model: self.model.clone().unwrap_or_default(),
            lens: None,
            unique_image_id: None,
            rating: None,
        };
        let mut exif_ifd = DirectoryWriter::new();
        // Orientation, ModifyDate, Copyright, Artist and the GPS IFD go to
        // `root`; everything else to `exif_ifd`.
        raw_meta
            .write_exif_tags(tiff, root, &mut exif_ifd)
            .map_err(tiff_err)?;
        if let Some(make) = &self.make {
            root.add_tag(ExifTag::Make, make.as_str());
        }
        if let Some(model) = &self.model {
            root.add_tag(ExifTag::Model, model.as_str());
        }
        root.add_tag(ExifTag::Software, SOFTWARE);
        for (&tag, value) in &self.extra {
            exif_ifd.add_untyped_tag(tag, value.clone());
        }
        exif_ifd.add_tag_undefined(ExifTag::ExifVersion, b"0232".to_vec());
        exif_ifd.add_tag(ExifTag::ExifImageWidth, width);
        exif_ifd.add_tag(ExifTag::ExifImageHeight, height);
        let exif_offset = exif_ifd.build(tiff).map_err(tiff_err)?;
        root.add_tag(ExifTag::ExifOffset, [exif_offset]);
        Ok(())
    }
}

fn tiff_err(e: rawler::formats::tiff::TiffError) -> anyhow::Error {
    anyhow!("writing EXIF: {e}")
}

/// rawler trims strings, so padded fields like an unset
/// `LensSerialNumber` come out as `""`. Don't write those.
fn drop_empty_strings(exif: &mut Exif) {
    let gps = exif.gps.as_mut();
    let fields = [
        &mut exif.copyright,
        &mut exif.artist,
        &mut exif.date_time_original,
        &mut exif.create_date,
        &mut exif.modify_date,
        &mut exif.offset_time,
        &mut exif.offset_time_original,
        &mut exif.offset_time_digitized,
        &mut exif.sub_sec_time,
        &mut exif.sub_sec_time_original,
        &mut exif.sub_sec_time_digitized,
        &mut exif.owner_name,
        &mut exif.serial_number,
        &mut exif.lens_serial_number,
        &mut exif.lens_make,
        &mut exif.lens_model,
        &mut exif.user_comment,
    ];
    let gps_fields = gps.into_iter().flat_map(|g| {
        [
            &mut g.gps_latitude_ref,
            &mut g.gps_longitude_ref,
            &mut g.gps_satellites,
            &mut g.gps_status,
            &mut g.gps_measure_mode,
            &mut g.gps_speed_ref,
            &mut g.gps_track_ref,
            &mut g.gps_img_direction_ref,
            &mut g.gps_map_datum,
            &mut g.gps_dest_latitude_ref,
            &mut g.gps_dest_longitude_ref,
            &mut g.gps_dest_bearing_ref,
            &mut g.gps_dest_distance_ref,
            &mut g.gps_date_stamp,
        ]
    });
    for field in fields.into_iter().chain(gps_fields) {
        if field.as_deref().is_some_and(|s| s.trim().is_empty()) {
            *field = None;
        }
    }
}

/// Read the metadata of `path`. Best effort: returns `None` if the file
/// has no metadata or it can't be read, and never panics (rawler's
/// metadata parsers aren't panic-free, and the GUI calls this on its UI
/// thread).
pub fn read_metadata(path: &Path) -> Option<ImageMetadata> {
    std::panic::catch_unwind(|| read_metadata_impl(path))
        .ok()
        .flatten()
        .filter(|m| !m.is_empty())
}

fn read_metadata_impl(path: &Path) -> Option<ImageMetadata> {
    let ext = extension(path);
    if is_raw_extension(&ext) {
        return read_raw_metadata(path);
    }
    if matches!(ext.as_str(), "tif" | "tiff") {
        // A TIFF file is its own EXIF container: parse its IFD0 directly.
        // Buffered, not read whole: float TIFFs can be hundreds of MiB.
        let mut file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
        return parse_tiff_metadata(&mut file);
    }
    // JPEG, PNG, WebP: the decoder hands out the raw EXIF block.
    let reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    let mut decoder = reader.into_decoder().ok()?;
    let blob = image::ImageDecoder::exif_metadata(&mut decoder).ok()??;
    parse_tiff_metadata(&mut Cursor::new(blob))
}

/// Metadata of a camera RAW file. The EXIF fields come from rawler,
/// which knows every format's quirks and resolves the lens from its
/// database. Make, model and [`EXTRA_EXIF_TAGS`] come from the file's own
/// TIFF-structured EXIF block when there is one (see
/// [`raw_container_metadata`]), so the names match the camera's own
/// JPEGs; otherwise rawler's cleaned-up names are used.
fn read_raw_metadata(path: &Path) -> Option<ImageMetadata> {
    let src = rawler::rawsource::RawSource::new(path).ok()?;
    let raw = rawler::get_decoder(&src)
        .and_then(|d| d.raw_metadata(&src, &RawDecodeParams::default()))
        .ok()?;
    let container = raw_container_metadata(src.buf()).unwrap_or_default();
    let non_empty = |s: String| Some(s).filter(|s| !s.trim().is_empty());
    let (make, model) = match container {
        ImageMetadata {
            make: Some(make),
            model: Some(model),
            ..
        } => (Some(make), Some(model)),
        _ => (non_empty(raw.make), non_empty(raw.model)),
    };
    Some(ImageMetadata {
        make,
        model,
        exif: raw.exif,
        extra: container.extra,
    })
}

/// How far into a non-TIFF RAW to look for an embedded EXIF block.
const EXIF_SEARCH_LIMIT: usize = 4 << 20;

/// The TIFF-structured EXIF of a RAW file: the file itself for TIFF-based
/// formats (NEF, CR2, ARW, DNG, PEF, ...), otherwise the first `Exif\0\0`
/// block near the start, as in the embedded preview JPEG of a Fuji RAF.
pub(crate) fn raw_container_metadata(buf: &[u8]) -> Option<ImageMetadata> {
    parse_tiff_metadata(&mut Cursor::new(buf)).or_else(|| {
        let head = &buf[..buf.len().min(EXIF_SEARCH_LIMIT)];
        let start = head.windows(6).position(|w| w == b"Exif\0\0")? + 6;
        parse_tiff_metadata(&mut Cursor::new(&buf[start..]))
    })
}

/// Parse a TIFF structure (a TIFF file or an EXIF block) starting at the
/// reader's offset 0.
fn parse_tiff_metadata<R: Read + Seek>(reader: &mut R) -> Option<ImageMetadata> {
    let sub_tags = [
        TiffCommonTag::ExifIFDPointer.into(),
        ExifTag::GPSInfo.into(),
    ];
    // Only IFD0 carries camera metadata; don't follow the IFD chain.
    let ifd = IFD::new_root_with_correction(reader, 0, 0, 0, 1, &sub_tags).ok()?;
    let exif = Exif::new(&ifd).ok()?;
    let ascii = |tag: ExifTag| {
        ifd.get_entry(tag)
            .and_then(|e| e.value.as_string())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let exif_ifd = ifd.get_sub_ifd(ExifTag::ExifOffset);
    let extra = EXTRA_EXIF_TAGS
        .iter()
        .filter_map(|&tag| {
            let entry = exif_ifd?.get_entry(tag)?;
            Some((tag.into(), entry.value.clone()))
        })
        .collect();
    Some(ImageMetadata {
        make: ascii(ExifTag::Make),
        model: ascii(ExifTag::Model),
        exif,
        extra,
    })
}
