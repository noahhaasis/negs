//! Listing a folder of photographs as `PhotoFolder` does
//! (`PhotoBrowser.swift`): which files are photographs, their order, the
//! sidecars beside them and each RAW's camera JPEG.

use crate::sidecar;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// RAW files (`EditorModel.rawExtensions`).
pub const RAW_EXTENSIONS: [&str; 30] = [
    "dng", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef", "ptx", "srw", "x3f", "iiq", "3fr", "ari", "bay", "crw",
    "dcr", "drf", "erf", "fff", "k25", "kdc", "mef", "mos", "mrw", "rwl",
];
/// Other photographs Negs develops.
pub const IMAGE_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "tif", "tiff"];
/// The camera's own picture beside a RAW of the same name. HEIF is only
/// ever a camera picture: Negs doesn't develop it.
pub const CAMERA_PICTURE_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "heic", "heif", "hif"];

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Whether Negs lists the file as a photograph.
pub fn is_photo(path: &Path) -> bool {
    let ext = extension(path);
    IMAGE_EXTENSIONS.contains(&ext.as_str()) || RAW_EXTENSIONS.contains(&ext.as_str())
}

pub fn is_raw(path: &Path) -> bool {
    RAW_EXTENSIONS.contains(&extension(path).as_str())
}

pub fn is_camera_picture(path: &Path) -> bool {
    CAMERA_PICTURE_EXTENSIONS.contains(&extension(path).as_str())
}

/// A file as listed: its size and date, so a later listing can tell what
/// changed without reading it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PhotoFile {
    pub path: PathBuf,
    pub name: String,
    pub modified: Option<SystemTime>,
    pub size: Option<u64>,
}

impl PhotoFile {
    pub fn new(path: PathBuf, metadata: Option<&std::fs::Metadata>) -> PhotoFile {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let metadata = metadata.cloned().or_else(|| std::fs::metadata(&path).ok());
        PhotoFile {
            name,
            modified: metadata.as_ref().and_then(|m| m.modified().ok()),
            size: metadata.as_ref().map(|m| m.len()),
            path,
        }
    }
}

/// One folder listing: its photographs in name order, the sidecars beside
/// them (by the photograph's file name), and, when pairing, each RAW's camera
/// picture, which is then not a photograph of its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing {
    pub photos: Vec<PhotoFile>,
    pub sidecars: HashMap<String, PhotoFile>,
    /// RAW → its camera JPEG (or HEIF).
    pub companions: HashMap<PathBuf, PathBuf>,
}

#[cfg(windows)]
fn hidden(_name: &str, metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    _name.starts_with('.') || metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}

#[cfg(not(windows))]
fn hidden(name: &str, _metadata: &std::fs::Metadata) -> bool {
    name.starts_with('.')
}

/// Lists `directory` (`PhotoFolder.listing`). Hidden files, such as a
/// sidecar's temporary file, are skipped, and so is anything that isn't a
/// regular file.
pub fn listing(directory: &Path, pairing: bool) -> io::Result<Listing> {
    let mut listing = Listing::default();
    let mut heifs: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = directory.join(&name);
        let ext = extension(&path);
        let is_sidecar = ext == sidecar::EXTENSION;
        let photo = IMAGE_EXTENSIONS.contains(&ext.as_str()) || RAW_EXTENSIONS.contains(&ext.as_str());
        let heif = pairing && !photo && CAMERA_PICTURE_EXTENSIONS.contains(&ext.as_str());
        if !(is_sidecar || heif || photo) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else { continue };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) || hidden(&name, &metadata) {
            continue;
        }
        if heif {
            heifs.push(path);
            continue;
        }
        let file = PhotoFile::new(path, Some(&metadata));
        if is_sidecar {
            let key = file.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            listing.sidecars.insert(key, file);
        } else {
            listing.photos.push(file);
        }
    }
    listing.photos.sort_by(|a, b| standard_compare(&a.name, &b.name));
    if pairing {
        pair(&mut listing, heifs);
    }
    Ok(listing)
}

/// The name a RAW and its camera picture share: `img_0412` for both
/// `IMG_0412.CR3` and `IMG_0412.jpg`.
fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Takes each RAW's camera picture out of the photographs: the first in
/// name order with the RAW's name, unless it has a sidecar (ratings or
/// edits of its own), which keeps it a photograph.
fn pair(listing: &mut Listing, mut heifs: Vec<PathBuf>) {
    let mut raws: HashMap<String, PathBuf> = HashMap::new();
    for photo in &listing.photos {
        if is_raw(&photo.path) {
            raws.entry(stem(&photo.path)).or_insert_with(|| photo.path.clone());
        }
    }
    if raws.is_empty() {
        return;
    }
    heifs.sort_by(|a, b| standard_compare(&a.file_name().unwrap_or_default().to_string_lossy(), &b.file_name().unwrap_or_default().to_string_lossy()));
    let pictures: Vec<PathBuf> = listing.photos.iter().map(|p| p.path.clone()).filter(|p| is_camera_picture(p)).chain(heifs).collect();
    let mut taken: Vec<PathBuf> = Vec::new();
    for picture in pictures {
        let Some(raw) = raws.get(&stem(&picture)) else { continue };
        let name = picture.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if listing.companions.contains_key(raw) || listing.sidecars.contains_key(&name) {
            continue;
        }
        listing.companions.insert(raw.clone(), picture.clone());
        taken.push(picture);
    }
    if !taken.is_empty() {
        listing.photos.retain(|p| !taken.contains(&p.path));
    }
}

/// The RAW a camera picture goes with, when pairing takes it: what opening
/// it opens instead (`PhotoFolder.pairedRAW`).
pub fn paired_raw(picture: &Path) -> Option<PathBuf> {
    if !is_camera_picture(picture) {
        return None;
    }
    let listing = listing(picture.parent()?, true).ok()?;
    let name = picture.file_name()?;
    listing.companions.iter().find(|(_, p)| p.file_name() == Some(name)).map(|(raw, _)| raw.clone())
}

// MARK: Name order

/// Where a character sorts before letters, as in the Unicode collation
/// table Finder uses: spaces, then punctuation and symbols in this order,
/// then digits and letters.
const PUNCTUATION: &str = "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Primary {
    Space,
    Punctuation(usize),
    Other(u32),
    /// A run of digits by value: its length without leading zeros, then the digits.
    Number(usize, String),
    Letter(char),
}

/// An accented Latin letter's base letter and accent, or the letter itself.
fn base_letter(c: char) -> (char, u32) {
    const TABLE: &[(char, &str)] = &[
        ('a', "àáâãäåāăą"),
        ('c', "çćĉċč"),
        ('d', "ďđ"),
        ('e', "èéêëēĕėęě"),
        ('g', "ĝğġģ"),
        ('h', "ĥħ"),
        ('i', "ìíîïĩīĭįı"),
        ('j', "ĵ"),
        ('k', "ķ"),
        ('l', "ĺļľŀł"),
        ('n', "ñńņňŉ"),
        ('o', "òóôõöøōŏő"),
        ('r', "ŕŗř"),
        ('s', "śŝşš"),
        ('t', "ţťŧ"),
        ('u', "ùúûüũūŭůűų"),
        ('w', "ŵ"),
        ('y', "ýÿŷ"),
        ('z', "źżž"),
    ];
    let lower = c.to_lowercase().next().unwrap_or(c);
    for (base, accented) in TABLE {
        if let Some(position) = accented.chars().position(|a| a == lower) {
            return (*base, position as u32 + 1);
        }
    }
    (lower, 0)
}

struct Element {
    primary: Primary,
    accent: u32,
    /// Upper case sorts after lower.
    upper: bool,
    /// Leading zeros of a number.
    zeros: usize,
}

fn elements(name: &str) -> Vec<Element> {
    let chars: Vec<char> = name.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            let trimmed = run.trim_start_matches('0').to_string();
            out.push(Element { zeros: run.len() - trimmed.len(), primary: Primary::Number(trimmed.len(), trimmed), accent: 0, upper: false });
            continue;
        }
        i += 1;
        let primary = if c.is_whitespace() {
            Primary::Space
        } else if let Some(rank) = PUNCTUATION.find(c) {
            Primary::Punctuation(rank)
        } else if c.is_alphabetic() {
            let (base, accent) = base_letter(c);
            out.push(Element { primary: Primary::Letter(base), accent, upper: c.is_uppercase(), zeros: 0 });
            continue;
        } else {
            Primary::Other(c as u32)
        };
        out.push(Element { primary, accent: 0, upper: false, zeros: 0 });
    }
    out
}

/// Finder's order (`localizedStandardCompare`): numbers by value, case and
/// accents only when nothing else differs (lower case first), punctuation
/// before digits before letters. Exact for Latin file names; other scripts
/// sort by character.
pub fn standard_compare(a: &str, b: &str) -> Ordering {
    let (x, y) = (elements(a), elements(b));
    let primary = x.iter().map(|e| &e.primary).cmp(y.iter().map(|e| &e.primary));
    primary
        .then_with(|| x.iter().map(|e| e.accent).cmp(y.iter().map(|e| e.accent)))
        .then_with(|| x.iter().map(|e| e.upper).cmp(y.iter().map(|e| e.upper)))
        .then_with(|| x.iter().map(|e| e.zeros).cmp(y.iter().map(|e| e.zeros)))
        .then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finder_order() {
        // As `localizedStandardCompare` sorted them on macOS 26.
        let sorted = "_a.jpg|0.jpg|00.jpg|000.jpg|1.jpg|9.jpg|10.jpg|a 1.png|a_1.png|a-1.png|a.b.jpg|a.jpg|ä.jpg|Ä.jpg|a.png|a~.jpg|a1.png|a01.png|a001.png|A1.png|a1b2.jpg|a1b10.jpg|a2.png|a10.png|ab.jpg|b.jpg|B.JPG|DSCF5.JPG|DSCF5.NEF|DSCF6.dng|DSCF6.png|e.jpg|é.jpg|f.jpg|IMG_0412.CR3|IMG_0412.JPG|IMG_0413 (2).jpg|img_0413.jpg|IMG_9999.NEF|IMG_10000.NEF|IMG-0413.jpg|photo 2.jpg|photo 10.jpg|photo2.jpg|x,1.jpg|x!1.jpg|x(1).jpg|x[1].jpg|x@1.jpg|x#1.jpg|x+1.jpg|x0.jpg|xa.jpg|z.jpg|Z.jpg";
        let expected: Vec<&str> = sorted.split('|').collect();
        let mut names = expected.clone();
        names.reverse();
        names.sort_by(|a, b| standard_compare(a, b));
        assert_eq!(names, expected);
        // Shuffled differently, the same order.
        let mut names = expected.clone();
        names.sort_by(|a, b| b.cmp(a));
        names.sort_by(|a, b| standard_compare(a, b));
        assert_eq!(names, expected);
    }

    fn folder(tag: &str, names: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("negs-library-folder-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in names {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        dir
    }

    #[test]
    fn the_listing_finds_sidecars_but_not_their_temporary_files() {
        let dir = folder("sidecars", &["b.jpg", "a10.png", "a2.png", "notes.txt", "a2.png.negs", "B.JPG.NEGS", ".a10.png.negs.tmp", "orphan.cr3.negs"]);
        let found = listing(&dir, true).unwrap();
        assert_eq!(found.photos.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["a2.png", "a10.png", "b.jpg"]);
        let mut sidecars: Vec<&str> = found.sidecars.keys().map(String::as_str).collect();
        sidecars.sort();
        assert_eq!(sidecars, ["B.JPG", "a2.png", "orphan.cr3"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A RAW's camera JPEG (or HEIF) of the same name goes with it, unless
    /// it has a sidecar of its own; pairing off lists both.
    #[test]
    fn camera_jpegs_go_with_their_raws() {
        let names = [
            "DSCF1.RAF", "DSCF1.JPG", "dscf2.raf", "DSCF2.jpeg", "DSCF3.JPG", "DSCF4.CR3", "DSCF4.HIF", "DSCF5.NEF", "DSCF5.JPG", "DSCF5.JPG.negs", "DSCF6.png",
            "DSCF6.dng", "DSCF7.heic",
        ];
        let dir = folder("pairs", &names);
        let paired = listing(&dir, true).unwrap();
        assert_eq!(
            paired.photos.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["DSCF1.RAF", "dscf2.raf", "DSCF3.JPG", "DSCF4.CR3", "DSCF5.JPG", "DSCF5.NEF", "DSCF6.dng", "DSCF6.png"],
            "a JPEG without a RAW, a rated one and a PNG stay; HEIF is never a photograph"
        );
        let companions: HashMap<String, String> = paired
            .companions
            .iter()
            .map(|(raw, picture)| (raw.file_name().unwrap().to_string_lossy().into(), picture.file_name().unwrap().to_string_lossy().into()))
            .collect();
        assert_eq!(
            companions,
            HashMap::from([("DSCF1.RAF".into(), "DSCF1.JPG".into()), ("dscf2.raf".into(), "DSCF2.jpeg".into()), ("DSCF4.CR3".into(), "DSCF4.HIF".into())])
        );
        let separate = listing(&dir, false).unwrap();
        assert_eq!(separate.photos.len(), 10, "HEIF is not listed when not paired");
        assert!(separate.companions.is_empty());
        assert_eq!(paired_raw(&dir.join("DSCF1.JPG")), Some(dir.join("DSCF1.RAF")));
        assert_eq!(paired_raw(&dir.join("DSCF3.JPG")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
