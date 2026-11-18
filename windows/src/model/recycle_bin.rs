//! The Recycle Bin as the library's `Bin` (`PhotoTrash` on the Mac):
//! `IFileOperation` recycles each file as Explorer's Delete does, undoable
//! from the Recycle Bin too, and says where it went, so Undo can put it back.
//! A volume without a bin (a network share, a memory card) deletes for good,
//! which the library asks about first.

use std::cell::RefCell;
use std::rc::Rc;
use std::io;
use std::path::{Path, PathBuf};

use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::*;
use windows::core::{HRESULT, PCWSTR, implement};

use crate::platform::{pcwstr, wide};

// GetDriveTypeW's answers.
const DRIVE_FIXED: u32 = 3;

pub struct RecycleBin;

fn other(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

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

/// Hears where a recycled file went.
#[implement(IFileOperationProgressSink)]
struct Sink {
    heard: Rc<Heard>,
}

#[derive(Default)]
struct Heard {
    landed: RefCell<Option<PathBuf>>,
    result: RefCell<Option<HRESULT>>,
}

impl RecycleBin {
    /// The volume `folder` is on, as GetDriveTypeW wants it ("C:\").
    fn volume(folder: &Path) -> Option<Vec<u16>> {
        let path = wide(folder);
        let mut volume = [0u16; 261];
        unsafe { GetVolumePathNameW(pcwstr(&path), &mut volume).ok()? };
        let end = volume.iter().position(|&c| c == 0).unwrap_or(volume.len());
        let mut out = volume[..end].to_vec();
        out.push(0);
        Some(out)
    }
}

impl negs_library::Bin for RecycleBin {
    fn name(&self) -> &str {
        "Recycle Bin"
    }

    /// Fixed disks, internal and external, have one. Memory cards, other
    /// removable media and network shares don't: Windows deletes there at
    /// once, even when asked to recycle.
    fn available(&self, folder: &Path) -> bool {
        let Some(volume) = Self::volume(folder) else { return false };
        unsafe { GetDriveTypeW(PCWSTR(volume.as_ptr())) == DRIVE_FIXED }
    }

    fn trash(&mut self, file: &Path) -> io::Result<PathBuf> {
        unsafe {
            let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL).map_err(other)?;
            // FOF_WANTNUKEWARNING: a file too big for the bin (or a bin
            // turned off) is never deleted for good without Windows asking.
            operation
                .SetOperationFlags(FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOF_WANTNUKEWARNING | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOFX_EARLYFAILURE)
                .map_err(other)?;
            let _ = operation.SetOwnerWindow(crate::ui::main_hwnd());
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
            // Recycled: the bin's own copy, `$R…`. Without it the file was
            // deleted for good, and Undo can't bring it back.
            heard.landed.borrow().clone().ok_or_else(|| other("Windows deleted it instead of moving it to the Recycle Bin."))
        }
    }

    /// As the Recycle Bin's Restore does: the `$R…` file goes back under its
    /// name, and its `$I…` record goes.
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
    fn StartOperations(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, _: HRESULT) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreRenameItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostRenameItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR, _: HRESULT, _: Option<&IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreMoveItem(&self, _: u32, _: Option<&IShellItem>, _: Option<&IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostMoveItem(&self, _: u32, _: Option<&IShellItem>, _: Option<&IShellItem>, _: &PCWSTR, _: HRESULT, _: Option<&IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreCopyItem(&self, _: u32, _: Option<&IShellItem>, _: Option<&IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostCopyItem(&self, _: u32, _: Option<&IShellItem>, _: Option<&IShellItem>, _: &PCWSTR, _: HRESULT, _: Option<&IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreDeleteItem(&self, _: u32, _: Option<&IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostDeleteItem(&self, _: u32, _: Option<&IShellItem>, result: HRESULT, created: Option<&IShellItem>) -> windows::core::Result<()> {
        *self.heard.result.borrow_mut() = Some(result);
        *self.heard.landed.borrow_mut() = created.and_then(item_path);
        Ok(())
    }
    fn PreNewItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostNewItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR, _: &PCWSTR, _: u32, _: HRESULT, _: Option<&IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, _: u32, _: u32) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use negs_library::Bin;

    /// A scratch file goes to the Recycle Bin and comes back byte for byte.
    #[test]
    fn a_file_goes_to_the_recycle_bin_and_back() {
        crate::thumbnail::com_thread();
        let folder = std::env::temp_dir().join(format!("negs-bin-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("Scratch.negs");
        std::fs::write(&file, b"{\"scratch\":true}").unwrap();
        let mut bin = RecycleBin;
        assert_eq!(bin.name(), "Recycle Bin");
        if !bin.available(&folder) {
            eprintln!("{} has no Recycle Bin: skipped", folder.display());
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

