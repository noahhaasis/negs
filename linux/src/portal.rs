//! Sandbox integration: document-portal identity and host-program environment.
use gtk::{gio, glib, prelude::*};
use std::{
    ffi::{CString, OsString},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf},
};

/// The document ID and the components after it, for a path in a document
/// portal mount (`/run/user/UID/doc/ID/...` or `/run/flatpak/doc/ID/...`).
pub fn document(path: &Path) -> Option<(String, PathBuf)> {
    let mut components = path.components();
    if components.next() != Some(Component::RootDir) {
        return None;
    }
    let names: Vec<_> = components.map(|c| c.as_os_str()).collect();
    let is = |name: &std::ffi::OsStr, expected: &str| name.as_bytes() == expected.as_bytes();
    let rest = match names.as_slice() {
        [run, user, _uid, doc, rest @ ..] if is(run, "run") && is(user, "user") && is(doc, "doc") => {
            rest
        }
        [run, flatpak, doc, rest @ ..]
            if is(run, "run") && is(flatpak, "flatpak") && is(doc, "doc") =>
        {
            rest
        }
        _ => return None,
    };
    let (id, rest) = rest.split_first()?;
    let id = id.to_str()?;
    if id == "by-app" || rest.is_empty() {
        return None;
    }
    Some((id.to_owned(), rest.iter().collect()))
}

/// The file's path on the host, for a document-portal path. None outside the
/// portal or when the portal cannot say. Identity only, never authority.
pub fn host_path(path: &Path) -> Option<PathBuf> {
    let (id, rest) = document(path)?;
    if let Some(host) = xattr_host_path(path) {
        return Some(host);
    }
    // The document's host path names the first component under the ID: the
    // granted file itself, or a granted folder whose contents follow.
    let base = documents_host_path(&id)?;
    let mut components = rest.components();
    components.next()?;
    Some(base.join(components.as_path()))
}

fn xattr_host_path(path: &Path) -> Option<PathBuf> {
    unsafe extern "C" {
        fn getxattr(path: *const i8, name: *const i8, value: *mut u8, size: usize) -> isize;
    }
    let file = CString::new(path.as_os_str().as_bytes()).ok()?;
    let name = c"user.document-portal.host-path";
    let mut buffer = vec![0u8; 4096];
    // SAFETY: both strings are NUL-terminated and the buffer length is exact.
    let length = unsafe {
        getxattr(file.as_ptr(), name.as_ptr(), buffer.as_mut_ptr(), buffer.len())
    };
    if length <= 0 {
        return None;
    }
    buffer.truncate(length as usize);
    while buffer.last() == Some(&0) {
        buffer.pop();
    }
    (!buffer.is_empty()).then(|| PathBuf::from(OsString::from_vec(buffer)))
}

fn documents_host_path(id: &str) -> Option<PathBuf> {
    let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>).ok()?;
    let reply = bus
        .call_sync(
            Some("org.freedesktop.portal.Documents"),
            "/org/freedesktop/portal/documents",
            "org.freedesktop.portal.Documents",
            "GetHostPaths",
            Some(&(vec![id.to_owned()],).to_variant()),
            glib::VariantTy::new("(a{say})").ok(),
            gio::DBusCallFlags::NONE,
            2000,
            None::<&gio::Cancellable>,
        )
        .ok()?;
    let paths = reply.child_value(0);
    let entry = paths.iter().find(|entry| entry.child_value(0).str() == Some(id))?;
    let mut bytes = entry.child_value(1).fixed_array::<u8>().ok()?.to_vec();
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    (!bytes.is_empty()).then(|| PathBuf::from(OsString::from_vec(bytes)))
}

/// True unless the destination is provably a different file from the source.
/// Different document-portal paths can name one host file, and their inode
/// numbers need not match; when the portal cannot identify both, an existing
/// destination is treated as possibly the original.
pub fn may_be_same_file(source: &Path, destination: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    if source == destination {
        return true;
    }
    let Ok(target) = std::fs::metadata(destination) else {
        return false;
    };
    if let Ok(original) = std::fs::metadata(source)
        && original.dev() == target.dev()
        && original.ino() == target.ino()
    {
        return true;
    }
    let portal = document(source).is_some() || document(destination).is_some();
    if !portal {
        return false;
    }
    let host = |path: &Path| {
        if document(path).is_some() {
            host_path(path)
        } else {
            Some(path.to_path_buf())
        }
    };
    match (host(source), host(destination)) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// A friendly name for a path shown to the user: the host path for portal
/// documents when the portal provides it.
pub fn display(path: &Path) -> String {
    host_path(path)
        .unwrap_or_else(|| path.to_path_buf())
        .display()
        .to_string()
}

/// Bundle-only variables a portable launcher sets for Negs. Host programs
/// must start with the user's original values, saved as NEGS_ORIG_<NAME>.
const BUNDLE_VARIABLES: &[&str] = &[
    "XDG_DATA_DIRS",
    "GTK_PATH",
    "GTK_EXE_PREFIX",
    "GTK_DATA_PREFIX",
    "GDK_PIXBUF_MODULE_FILE",
    "GIO_EXTRA_MODULES",
    "GSETTINGS_SCHEMA_DIR",
    "LD_LIBRARY_PATH",
];

/// A launch context for host programs that restores the launcher's saved
/// environment, so they never load the AppImage's bundled libraries.
pub fn host_launch_context(display: &gtk::gdk::Display) -> gio::AppLaunchContext {
    let context = display.app_launch_context();
    if std::env::var_os("NEGS_ORIG_SAVED").is_some() {
        for name in BUNDLE_VARIABLES {
            match std::env::var_os(format!("NEGS_ORIG_{name}")) {
                Some(value) if !value.is_empty() => context.setenv(name, value),
                _ => context.unsetenv(name),
            }
        }
        context.unsetenv("NEGS_ORIG_SAVED");
        for name in BUNDLE_VARIABLES {
            context.unsetenv(format!("NEGS_ORIG_{name}"));
        }
        for name in ["NEGS_DATA_DIR", "NEGS_PACKAGE_ROOT", "APPDIR", "APPIMAGE", "ARGV0", "OWD"] {
            context.unsetenv(name);
        }
    }
    context.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_paths_split_at_the_portal_id() {
        assert_eq!(
            document(Path::new("/run/user/1000/doc/ab12/Trip/a.nef")),
            Some(("ab12".into(), PathBuf::from("Trip/a.nef")))
        );
        assert_eq!(
            document(Path::new("/run/flatpak/doc/cd34/b.jpg")),
            Some(("cd34".into(), PathBuf::from("b.jpg")))
        );
        assert_eq!(document(Path::new("/run/user/1000/doc/ab12")), None);
        assert_eq!(document(Path::new("/run/user/1000/doc/by-app/x/y")), None);
        assert_eq!(document(Path::new("/home/me/doc/ab12/a.jpg")), None);
    }
    #[test]
    fn native_paths_compare_by_inode() {
        let directory = std::env::temp_dir().join(format!("negs-portal-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source");
        let alias = directory.join("alias");
        let other = directory.join("other");
        std::fs::write(&source, [1]).unwrap();
        std::fs::write(&other, [1]).unwrap();
        let _ = std::fs::remove_file(&alias);
        std::fs::hard_link(&source, &alias).unwrap();
        assert!(may_be_same_file(&source, &alias));
        assert!(!may_be_same_file(&source, &other));
        assert!(!may_be_same_file(&source, &directory.join("missing")));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
