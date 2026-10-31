//! Identify DNG before trying a generic TIFF raster decoder. IFD0 can be a
//! perfectly decodable RGB thumbnail, not the photograph's sensor data.

/// Whether a classic TIFF's IFD0 contains DNGVersion (50706). DNG requires
/// this tag in IFD0, including when the RAW image lives in a SubIFD. Inspect
/// only the directory, without decoding pixels, trusting offsets or scanning
/// arbitrary image data for a byte signature. A malformed identified DNG must
/// fail RAW development, not fall back to its thumbnail.
pub fn is_dng(bytes: &[u8]) -> bool {
    fn identify(bytes: &[u8]) -> Option<bool> {
        let little = match bytes.get(..2)? {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };
        let u16_at = |at: usize| -> Option<u16> {
            let b = bytes.get(at..at.checked_add(2)?)?.try_into().ok()?;
            Some(if little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
        };
        let u32_at = |at: usize| -> Option<u32> {
            let b = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
            Some(if little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
        };
        if u16_at(2)? != 42 {
            return None; // DNG uses TIFF 6.0, not BigTIFF.
        }
        let ifd = usize::try_from(u32_at(4)?).ok()?;
        let count = usize::from(u16_at(ifd)?);
        let start = ifd.checked_add(2)?;
        let entries = bytes.get(start..start.checked_add(count.checked_mul(12)?)?)?;
        Some(entries.chunks_exact(12).any(|e| {
            let tag = [e[0], e[1]];
            (if little { u16::from_le_bytes(tag) } else { u16::from_be_bytes(tag) }) == 50706
        }))
    }
    identify(bytes).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(little: bool, tag: u16) -> Vec<u8> {
        let mut bytes = if little { b"II" } else { b"MM" }.to_vec();
        let u16b = |v: u16| if little { v.to_le_bytes() } else { v.to_be_bytes() };
        let u32b = |v: u32| if little { v.to_le_bytes() } else { v.to_be_bytes() };
        bytes.extend(u16b(42));
        bytes.extend(u32b(8));
        bytes.extend(u16b(1));
        bytes.extend(u16b(tag));
        bytes.extend(u16b(1));
        bytes.extend(u32b(4));
        bytes.extend([1, 4, 0, 0]);
        bytes.extend(u32b(0));
        bytes
    }

    #[test]
    fn identifies_dng_in_either_byte_order_not_ordinary_tiff() {
        for little in [true, false] {
            assert!(is_dng(&directory(little, 50706)));
            assert!(!is_dng(&directory(little, 256)));
        }
        assert!(!is_dng(b"not a TIFF\x12\xc6"));
        let mut raster = directory(true, 256);
        raster.extend([0x12, 0xc6]);
        assert!(!is_dng(&raster), "a signature outside IFD0 is not a DNG tag");
    }

    #[test]
    fn truncated_or_hostile_directories_do_not_panic() {
        let bytes = directory(true, 50706);
        for end in 0..22 {
            assert!(!is_dng(&bytes[..end]));
        }
        let mut invalid = bytes.clone();
        invalid[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(!is_dng(&invalid));
        invalid = bytes;
        invalid[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(!is_dng(&invalid));
        invalid[2..4].copy_from_slice(&43u16.to_le_bytes());
        assert!(!is_dng(&invalid));
    }
}
