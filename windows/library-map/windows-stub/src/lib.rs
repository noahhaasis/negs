//! The one Win32 call `state.rs` makes (film search's folding compare),
//! stood in for so the app's state compiles on any OS. Lower case only:
//! the mapping tests never search films.
#![allow(non_snake_case, non_upper_case_globals, clippy::missing_safety_doc)]

pub mod Win32 {
    pub mod Foundation {
        pub struct LPARAM(pub isize);
    }

    pub mod Globalization {
        pub struct Flag(pub u32);
        pub const FIND_FROMSTART: u32 = 0x0040_0000;
        pub const LINGUISTIC_IGNORECASE: Flag = Flag(0x10);
        pub const LINGUISTIC_IGNOREDIACRITIC: Flag = Flag(0x20);
        pub const LOCALE_NAME_INVARIANT: &str = "";

        pub unsafe fn FindNLSStringEx(
            _locale: &str,
            _flags: u32,
            text: &[u16],
            word: &[u16],
            _found: Option<()>,
            _version: Option<()>,
            _reserved: Option<()>,
            _handle: super::Foundation::LPARAM,
        ) -> i32 {
            let (text, word) = (String::from_utf16_lossy(text).to_lowercase(), String::from_utf16_lossy(word).to_lowercase());
            text.find(&word).map_or(-1, |at| at as i32)
        }
    }
}
