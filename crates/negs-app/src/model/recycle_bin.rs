//! OS trash / Recycle Bin as the library's `Bin`:
//! - Windows: `IFileOperation` recycles each file as Explorer's Delete does,
//!   undoable from the Recycle Bin too, and says where it went so Undo can put it back.
//! - macOS / Linux: moves files to the user's Trash folder (or a scratch trash
//!   folder during unit tests) with unique names, and moves them back on Undo.

use std::io;
use std::path::{Path, PathBuf};

pub struct RecycleBin;

fn other(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
    use windows::Win32::UI::Shell::*;
    use windows::core::{HRESULT, PCWSTR, implement};

    use crate::platform::{pcwstr, wide};

    const DRIVE_FIXED: u32 = 3;

    fn shell_item(path: &Path) -> io::Result<IShellItem> {
        let wide_path = wide(path);
        unsafe { SHCreateItemFromParsingName(pcwstr(&wide_path), None).map_err(other) }
    }

    fn item_path(item: &IShellItem) -> Option<PathBuf> {
        unsafe {
            let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let path = name.to_string().ok().map(PathBuf::from);
            CoTaskMemFree(Some(name.0 as *const _));
            path
        }
    }

    #[implement(IFileOperationProgressSink)]
    struct Sink {
        heard: Rc<Heard>,
    }

    #[derive(Default)]
    struct Heard {
        landed: RefCell<Option<PathBuf>>,
        result: RefCell<Option<HRESULT>>,
    }

    fn volume(folder: &Path) -> Option<Vec<u16>> {
        let path = wide(folder);
        let mut volume = [0u16; 261];
        unsafe { GetVolumePathNameW(pcwstr(&path), &mut volume).ok()? };
        let end = volume.iter().position(|&c| c == 0).unwrap_or(volume.len());
        let mut out = volume[..end].to_vec();
        out.push(0);
        Some(out)
    }

    impl negs_library::Bin for RecycleBin {
        fn name(&self) -> &str {
            "Recycle Bin"
        }

        fn available(&self, folder: &Path) -> bool {
            let Some(volume) = volume(folder) else { return false };
            unsafe { GetDriveTypeW(PCWSTR(volume.as_ptr())) == DRIVE_FIXED }
        }

        fn trash(&mut self, file: &Path) -> io::Result<PathBuf> {
            unsafe {
                let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL).map_err(other)?;
                operation
                    .SetOperationFlags(FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOF_WANTNUKEWARNING | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOFX_EARLYFAILURE)
                    .map_err(other)?;
                let _ = operation.SetOwnerWindow(HWND(crate::host::owner_window_handle() as *mut _));
                let heard = Rc::new(Heard::default());
                let sink: IFileOperationProgressSink = Sink { heard: heard.clone() }.into();
                operation.DeleteItem(&shell_item(file)?, &sink).map_err(other)?;
                operation.PerformOperations().map_err(other)?;
                if operation.GetAnyOperationsAborted().map_err(other)?.as_bool() {
                    return Err(other("Moving it to the Recycle Bin was stopped."));
                }
                if let Some(result) = *heard.result.borrow() {
                    result.ok().map_err(other)?;
                }
                if file.exists() {
                    return Err(other("It is still in its folder."));
                }
                heard.landed.borrow().clone().ok_or_else(|| other("Windows deleted it instead of moving it to the Recycle Bin."))
            }
        }

        fn put_back(&mut self, trashed: &Path, original: &Path) -> io::Result<()> {
            if original.exists() {
                return Err(other(format!("“{}” is already there.", original.file_name().unwrap_or_default().to_string_lossy())));
            }
            std::fs::rename(trashed, original)?;
            if let Some(name) = trashed.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix("$R")) {
                let _ = std::fs::remove_file(trashed.with_file_name(format!("$I{name}")));
            }
            Ok(())
        }
    }

    impl IFileOperationProgressSink_Impl for Sink_Impl {
        fn StartOperations(&self) -> windows::core::Result<()> { Ok(()) }
        fn FinishOperations(&self, _: HRESULT) -> windows::core::Result<()> { Ok(()) }
        fn PreRenameItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR) -> windows::core::Result<()> { Ok(()) }
        fn PostRenameItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR, _: HRESULT, _: Option<&IShellItem>) -> windows::core::Result<()> { Ok(()) }
        fn PreMoveItem(&self, _: u32, _: Option<&IShellItem>, _: Option<&IShellItem>, _: &PCWSTR) -> windows::core::Result<()> { Ok(()) }
        fn PostMoveItem(&self, _: u32, _: Option<&IShellItem>, _: Option<&IShellItem>, _: &PCWSTR, _: HRESULT, _: Option<&IShellItem>) -> windows::core::Result<()> { Ok(()) }
        fn PreCopyItem(&self, _: u32, _: Option<&IShellItem>, _: Option<&IShellItem>, _: &PCWSTR) -> windows::core::Result<()> { Ok(()) }
        fn PostCopyItem(&self, _: u32, _: Option<&IShellItem>, _: Option<&IShellItem>, _: &PCWSTR, _: HRESULT, _: Option<&IShellItem>) -> windows::core::Result<()> { Ok(()) }
        fn PreDeleteItem(&self, _: u32, _: Option<&IShellItem>) -> windows::core::Result<()> { Ok(()) }
        fn PostDeleteItem(&self, _: u32, _: Option<&IShellItem>, result: HRESULT, created: Option<&IShellItem>) -> windows::core::Result<()> {
            *self.heard.result.borrow_mut() = Some(result);
            *self.heard.landed.borrow_mut() = created.and_then(item_path);
            Ok(())
        }
        fn PreNewItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR) -> windows::core::Result<()> { Ok(()) }
        fn PostNewItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR, _: &PCWSTR, _: u32, _: HRESULT, _: Option<&IShellItem>) -> windows::core::Result<()> { Ok(()) }
        fn UpdateProgress(&self, _: u32, _: u32) -> windows::core::Result<()> { Ok(()) }
        fn ResetTimer(&self) -> windows::core::Result<()> { Ok(()) }
        fn PauseTimer(&self) -> windows::core::Result<()> { Ok(()) }
        fn ResumeTimer(&self) -> windows::core::Result<()> { Ok(()) }
    }
}

#[cfg(not(windows))]
mod unix {
    use super::*;

    fn trash_dir() -> PathBuf {
        if let Some(dir) = std::env::var_os("NEGS_TRASH_DIR") {
            return PathBuf::from(dir);
        }
        if cfg!(test) {
            return std::env::temp_dir().join(format!("negs-test-trash-{}", std::process::id()));
        }
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        #[cfg(target_os = "macos")]
        {
            home.join(".Trash")
        }
        #[cfg(not(target_os = "macos"))]
        {
            std::env::var_os("XDG_DATA_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share"))
                .join("Trash/files")
        }
    }

    fn unique_dest(trash: &Path, file: &Path) -> PathBuf {
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        let candidate = trash.join(name.as_ref());
        if !candidate.exists() {
            return candidate;
        }
        let stem = file.file_stem().unwrap_or_default().to_string_lossy();
        let ext = file.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        for n in 2..10_000 {
            let candidate = trash.join(format!("{stem} {n}{ext}"));
            if !candidate.exists() {
                return candidate;
            }
        }
        trash.join(format!("{stem}-{}{ext}", std::process::id()))
    }

    fn move_file(from: &Path, to: &Path) -> io::Result<()> {
        match std::fs::rename(from, to) {
            Ok(()) => Ok(()),
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                std::fs::copy(from, to)?;
                std::fs::remove_file(from)?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    impl negs_library::Bin for RecycleBin {
        fn name(&self) -> &str {
            "Trash"
        }

        fn available(&self, folder: &Path) -> bool {
            folder.exists()
        }

        fn trash(&mut self, file: &Path) -> io::Result<PathBuf> {
            let trash = trash_dir();
            std::fs::create_dir_all(&trash)?;
            let dest = unique_dest(&trash, file);
            move_file(file, &dest)?;
            Ok(dest)
        }

        fn put_back(&mut self, trashed: &Path, original: &Path) -> io::Result<()> {
            if original.exists() {
                return Err(other(format!("“{}” is already there.", original.file_name().unwrap_or_default().to_string_lossy())));
            }
            move_file(trashed, original)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use negs_library::Bin;

    /// A scratch file goes to the Recycle Bin / Trash and comes back byte for byte.
    #[test]
    fn a_file_goes_to_the_recycle_bin_and_back() {
        crate::thumbnail::com_thread();
        let folder = std::env::temp_dir().join(format!("negs-bin-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("Scratch.negs");
        std::fs::write(&file, b"{\"scratch\":true}").unwrap();
        let mut bin = RecycleBin;
        #[cfg(windows)]
        assert_eq!(bin.name(), "Recycle Bin");
        #[cfg(not(windows))]
        assert_eq!(bin.name(), "Trash");
        if !bin.available(&folder) {
            eprintln!("{} has no {}: skipped", folder.display(), bin.name());
            return;
        }
        let trashed = bin.trash(&file).unwrap();
        assert!(!file.exists());
        assert!(trashed.exists(), "{}", trashed.display());
        bin.put_back(&trashed, &file).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"{\"scratch\":true}");
        assert!(!trashed.exists());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
