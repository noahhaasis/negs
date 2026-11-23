//! Portable stand-ins for the few `windows` crate (0.58) items the frozen
//! Phase 4 oracles call, so they compile and run on macOS. Each keeps the
//! signature the oracle uses; the behaviour is the closest macOS has, and
//! where it differs from Windows it says so. The differential tests never
//! depend on those differences: they compare the core with these oracles
//! on what both platforms agree on.
#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types, clippy::missing_safety_doc)]

pub mod core {
    /// An HRESULT: `0x8007_xxxx` for a Win32 error code, as Windows wraps them.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct HRESULT(pub i32);

    /// A borrowed NUL-terminated UTF-16 string.
    #[derive(Clone, Copy, Debug)]
    pub struct PCWSTR(pub *const u16);

    impl PCWSTR {
        /// The text up to its NUL.
        pub unsafe fn text(self) -> String {
            let mut n = 0;
            while unsafe { *self.0.add(n) } != 0 {
                n += 1;
            }
            String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(self.0, n) })
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Error {
        code: HRESULT,
        message: String,
    }

    impl Error {
        pub fn new(code: HRESULT, message: impl Into<String>) -> Self {
            Self { code, message: message.into() }
        }

        pub fn code(&self) -> HRESULT {
            self.code
        }

        /// The system's text for the error. Windows' FormatMessageW text
        /// ends in ".\r\n"; macOS's strerror text has neither.
        pub fn message(&self) -> String {
            self.message.clone()
        }
    }

    pub type Result<T> = std::result::Result<T, Error>;
}

#[allow(non_snake_case)]
pub mod Win32 {
    pub mod Foundation {
        use crate::core::HRESULT;

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct WIN32_ERROR(pub u32);

        impl WIN32_ERROR {
            pub const fn to_hresult(self) -> HRESULT {
                HRESULT(if self.0 == 0 { 0 } else { ((self.0 & 0xFFFF) | 0x8007_0000) as i32 })
            }
        }

        pub const ERROR_FILE_NOT_FOUND: WIN32_ERROR = WIN32_ERROR(2);
        pub const ERROR_ACCESS_DENIED: WIN32_ERROR = WIN32_ERROR(5);
        pub const ERROR_SHARING_VIOLATION: WIN32_ERROR = WIN32_ERROR(32);
        pub const ERROR_LOCK_VIOLATION: WIN32_ERROR = WIN32_ERROR(33);
        pub const ERROR_FILE_EXISTS: WIN32_ERROR = WIN32_ERROR(80);
        pub const ERROR_ALREADY_EXISTS: WIN32_ERROR = WIN32_ERROR(183);

        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct LPARAM(pub isize);

        /// The Win32 error an errno stands for, as far as the oracles ask:
        /// EEXIST is ERROR_ALREADY_EXISTS, ENOENT ERROR_FILE_NOT_FOUND,
        /// EACCES/EPERM ERROR_ACCESS_DENIED, others pass through as is.
        /// macOS has no sharing or lock violations: a file another program
        /// has open can always be renamed.
        pub fn from_errno(errno: i32) -> WIN32_ERROR {
            match errno {
                17 => ERROR_ALREADY_EXISTS,
                2 => ERROR_FILE_NOT_FOUND,
                1 | 13 => ERROR_ACCESS_DENIED,
                other => WIN32_ERROR(0x1_0000 + other as u32),
            }
        }
    }

    pub mod Storage {
        pub mod FileSystem {
            use crate::Win32::Foundation::from_errno;
            use crate::core::{Error, PCWSTR, Result};

            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub struct FILE_FLAGS_AND_ATTRIBUTES(pub u32);
            pub const FILE_ATTRIBUTE_HIDDEN: FILE_FLAGS_AND_ATTRIBUTES = FILE_FLAGS_AND_ATTRIBUTES(2);

            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub struct MOVE_FILE_FLAGS(pub u32);
            pub const MOVEFILE_REPLACE_EXISTING: MOVE_FILE_FLAGS = MOVE_FILE_FLAGS(1);
            pub const MOVEFILE_WRITE_THROUGH: MOVE_FILE_FLAGS = MOVE_FILE_FLAGS(8);

            impl std::ops::BitOr for MOVE_FILE_FLAGS {
                type Output = Self;
                fn bitor(self, other: Self) -> Self {
                    Self(self.0 | other.0)
                }
            }

            unsafe extern "C" {
                fn renamex_np(from: *const std::ffi::c_char, to: *const std::ffi::c_char, flags: u32) -> i32;
            }
            const RENAME_EXCL: u32 = 0x4;

            /// rename(2): with MOVEFILE_REPLACE_EXISTING it replaces the
            /// target, without it renamex_np(RENAME_EXCL) fails with EEXIST
            /// as MoveFileExW fails with ERROR_ALREADY_EXISTS. Write-through
            /// is ignored (APFS renames are atomic; nothing is flushed).
            pub unsafe fn MoveFileExW(from: PCWSTR, to: PCWSTR, flags: MOVE_FILE_FLAGS) -> Result<()> {
                let (from, to) = unsafe { (from.text(), to.text()) };
                let c = |s: &str| std::ffi::CString::new(s).unwrap();
                let (f, t) = (c(&from), c(&to));
                let excl = if flags.0 & MOVEFILE_REPLACE_EXISTING.0 != 0 { 0 } else { RENAME_EXCL };
                if unsafe { renamex_np(f.as_ptr(), t.as_ptr(), excl) } == 0 {
                    return Ok(());
                }
                let error = std::io::Error::last_os_error();
                let code = from_errno(error.raw_os_error().unwrap_or(0));
                Err(Error::new(code.to_hresult(), error.to_string()))
            }
        }
    }

    pub mod Globalization {
        use crate::Win32::Foundation::LPARAM;
        use crate::core::PCWSTR;
        use icu_normalizer::properties::CanonicalCombiningClassMapBorrowed;
        use icu_normalizer::{ComposingNormalizerBorrowed, DecomposingNormalizerBorrowed};

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct NORM_FORM(pub i32);
        pub const NormalizationC: NORM_FORM = NORM_FORM(1);

        /// NormalizeString to NFC with ICU's data (Unicode 16). Windows'
        /// tables follow its own Unicode version, which can differ for
        /// characters newer than it. Asked without a buffer, it gives the
        /// length exactly (Windows gives an estimate, which the oracle
        /// handles).
        pub unsafe fn NormalizeString(_form: NORM_FORM, source: &[u16], destination: Option<&mut [u16]>) -> i32 {
            let text = String::from_utf16_lossy(source);
            let nfc: Vec<u16> = ComposingNormalizerBorrowed::new_nfc().normalize(&text).encode_utf16().collect();
            match destination {
                None => nfc.len() as i32,
                Some(out) if out.len() >= nfc.len() => {
                    out[..nfc.len()].copy_from_slice(&nfc);
                    nfc.len() as i32
                }
                Some(_) => -(nfc.len() as i32),
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct COMPARE_STRING_FLAGS(pub u32);
        pub const FIND_FROMSTART: u32 = 0x0040_0000;
        pub const LINGUISTIC_IGNORECASE: COMPARE_STRING_FLAGS = COMPARE_STRING_FLAGS(0x10);
        pub const LINGUISTIC_IGNOREDIACRITIC: COMPARE_STRING_FLAGS = COMPARE_STRING_FLAGS(0x20);
        pub const LOCALE_NAME_INVARIANT: PCWSTR = PCWSTR([0u16].as_ptr());

        /// Folded as the invariant locale's linguistic comparison folds:
        /// canonical decomposition, combining marks dropped, lower case.
        fn fold(text: &str) -> String {
            let marks = CanonicalCombiningClassMapBorrowed::new();
            DecomposingNormalizerBorrowed::new_nfd()
                .normalize(text)
                .chars()
                .filter(|&c| marks.get_u8(c) == 0 && !('\u{300}'..='\u{36f}').contains(&c))
                .flat_map(char::to_lowercase)
                .collect()
        }

        /// FindNLSStringEx from the start, ignoring case and diacritics:
        /// the folded word's position in the folded text (in folded UTF-8
        /// bytes, not the source's UTF-16 index), -1 when absent. Windows
        /// also folds a few letters (ß, æ, ligatures) linguistically; this
        /// doesn't. The oracles only ask whether it is >= 0.
        #[allow(clippy::too_many_arguments)]
        pub unsafe fn FindNLSStringEx(
            _locale: PCWSTR,
            _flags: u32,
            source: &[u16],
            value: &[u16],
            _found: Option<*mut i32>,
            _version: Option<*const u8>,
            _reserved: Option<*const std::ffi::c_void>,
            _handle: LPARAM,
        ) -> i32 {
            let (text, word) = (fold(&String::from_utf16_lossy(source)), fold(&String::from_utf16_lossy(value)));
            text.find(&word).map_or(-1, |i| i as i32)
        }
    }
}
