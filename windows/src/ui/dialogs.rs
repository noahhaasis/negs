//! Open and save dialogs, error and question dialogs, Settings and About.
//! Export's options live inside the save dialog (§20.4, decided): the file
//! type filter picks the format, `IFileDialogCustomize` adds Engine, Size and
//! the message.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::path::PathBuf;

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{GUID, Interface, PCWSTR, PWSTR, implement};

use super::App;
use crate::engine::ExportEngine;
use crate::model::{COLOR_SPACES, Edge, ExportSize, IMAGE_EXTENSIONS, OutputFormat, PIXEL_RANGE, PROPHOTO, QUALITY_RANGE, RAW_EXTENSIONS, SaveSettings};
use crate::platform::{pcwstr, wide};

// Each dialog remembers its own folder.
const OPEN_GUID: GUID = GUID::from_u128(0x6a3b_51f0_9c1e_4d2b_8f3a_0e5d_7c41_a001);
const EXPORT_GUID: GUID = GUID::from_u128(0x6a3b_51f0_9c1e_4d2b_8f3a_0e5d_7c41_a002);
const PREVIEW_GUID: GUID = GUID::from_u128(0x6a3b_51f0_9c1e_4d2b_8f3a_0e5d_7c41_a003);
const LUT_GUID: GUID = GUID::from_u128(0x6a3b_51f0_9c1e_4d2b_8f3a_0e5d_7c41_a004);

thread_local! {
    static TOOLTIP: Cell<isize> = const { Cell::new(0) };
    static TIPS: RefCell<Vec<(isize, Vec<u16>)>> = const { RefCell::new(Vec::new()) };
}

/// Sets `hwnd`'s tooltip, sharing one tooltip window.
pub fn tooltip(hwnd: HWND, text: &str) {
    unsafe {
        let tip = TOOLTIP.with(|t| {
            if t.get() == 0 {
                let tip = CreateWindowExW(
                    WS_EX_TOPMOST,
                    TOOLTIPS_CLASSW,
                    PCWSTR::null(),
                    WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    super::main_hwnd(),
                    None,
                    super::instance(),
                    None,
                )
                .unwrap_or_default();
                SendMessageW(tip, TTM_SETMAXTIPWIDTH, WPARAM(0), LPARAM(420));
                t.set(tip.0 as isize);
            }
            HWND(t.get() as *mut _)
        });
        let text = wide(text);
        let known = TIPS.with(|tips| {
            let mut tips = tips.borrow_mut();
            let entry = tips.iter_mut().find(|(h, _)| *h == hwnd.0 as isize);
            match entry {
                Some((_, existing)) if *existing == text => None,
                Some((_, existing)) => {
                    *existing = text.clone();
                    Some(true)
                }
                None => {
                    tips.push((hwnd.0 as isize, text.clone()));
                    Some(false)
                }
            }
        });
        let Some(known) = known else { return };
        // The text pointer stays valid: TIPS keeps it.
        let pointer = TIPS.with(|tips| tips.borrow().iter().find(|(h, _)| *h == hwnd.0 as isize).map(|(_, t)| t.as_ptr()).unwrap());
        let info = TTTOOLINFOW {
            cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
            uFlags: TTF_IDISHWND | TTF_SUBCLASS,
            hwnd: GetParent(hwnd).unwrap_or_default(),
            uId: hwnd.0 as usize,
            lpszText: PWSTR(pointer as *mut _),
            ..Default::default()
        };
        let message = if known { TTM_UPDATETIPTEXTW } else { TTM_ADDTOOLW };
        SendMessageW(tip, message, WPARAM(0), LPARAM(&info as *const _ as isize));
    }
}

fn task_dialog(owner: HWND, instruction: &str, content: &str, details: Option<&str>, icon: PCWSTR, buttons: &[(i32, &str)], flags: TASKDIALOG_FLAGS) -> i32 {
    let title = wide("Negs");
    let instruction = wide(instruction);
    let content = wide(content);
    let details = details.map(wide);
    let labels: Vec<Vec<u16>> = buttons.iter().map(|(_, l)| wide(l)).collect();
    let buttons: Vec<TASKDIALOG_BUTTON> = buttons.iter().zip(&labels).map(|((id, _), l)| TASKDIALOG_BUTTON { nButtonID: *id, pszButtonText: pcwstr(l) }).collect();
    let mut config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: owner,
        hInstance: super::instance(),
        dwFlags: flags | TDF_POSITION_RELATIVE_TO_WINDOW | TDF_SIZE_TO_CONTENT,
        pszWindowTitle: pcwstr(&title),
        pszMainInstruction: pcwstr(&instruction),
        pszContent: pcwstr(&content),
        cButtons: buttons.len() as u32,
        pButtons: if buttons.is_empty() { std::ptr::null() } else { buttons.as_ptr() },
        dwCommonButtons: if buttons.is_empty() { TDCBF_OK_BUTTON } else { TASKDIALOG_COMMON_BUTTON_FLAGS(0) },
        ..Default::default()
    };
    config.Anonymous1.pszMainIcon = icon;
    if let Some(details) = &details {
        config.pszExpandedInformation = pcwstr(details);
    }
    let mut pressed = 0;
    unsafe {
        let _ = TaskDialogIndirect(&config, Some(&mut pressed), None, None);
    }
    pressed
}

/// An error, with an exporter log as expandable details.
pub fn error(owner: HWND, message: &str) {
    let (first, rest) = message.split_once('\n').unwrap_or((message, ""));
    let details = (!rest.trim().is_empty()).then_some(rest.trim());
    task_dialog(owner, first, "", details, TD_WARNING_ICON, &[], TASKDIALOG_FLAGS(0));
}

/// True to quit and cancel the export.
pub fn confirm_quit_during_export(owner: HWND) -> bool {
    task_dialog(
        owner,
        "An export is still in progress.",
        "Quitting cancels it. Any existing file at the destination is left unchanged.",
        None,
        TD_WARNING_ICON,
        &[(100, "Continue exporting"), (101, "Quit")],
        TDF_USE_COMMAND_LINKS,
    ) == 101
}

pub fn about(owner: HWND) {
    let content = format!(
        "Version {}\n\nNegs is built on Andrea Volpato's spektrafilm, the spectral film simulation behind every stock, and turbasvin's spektrafilm-rs, the Rust port that runs it on the GPU.\n\nLicensed under the GNU General Public License, version 3.",
        env!("CARGO_PKG_VERSION")
    );
    task_dialog(owner, "Negs", &content, None, TD_INFORMATION_ICON, &[], TASKDIALOG_FLAGS(0));
}

/// Settings: the update preferences, like the Mac's Settings window.
pub fn settings(app: &mut App) {
    let configured = crate::updates::configured();
    if !configured {
        task_dialog(app.main, "Settings", "Updates are not configured for this build.", None, PCWSTR::null(), &[], TASKDIALOG_FLAGS(0));
        return;
    }
    let last = crate::updates::last_check().map_or("never".to_string(), |t| t);
    let title = wide("Negs");
    let instruction = wide("Settings");
    let content = wide(format!("Updates are installed only when you choose to restart Negs.\n\nLast checked: {last}"));
    let verification = wide("Automatically check for updates");
    let check = wide("Check for Updates…");
    let close = wide("Close");
    let buttons = [TASKDIALOG_BUTTON { nButtonID: 100, pszButtonText: pcwstr(&check) }, TASKDIALOG_BUTTON { nButtonID: 101, pszButtonText: pcwstr(&close) }];
    let automatic = crate::updates::automatic();
    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: app.main,
        dwFlags: TDF_POSITION_RELATIVE_TO_WINDOW | if automatic { TDF_VERIFICATION_FLAG_CHECKED } else { TASKDIALOG_FLAGS(0) },
        pszWindowTitle: pcwstr(&title),
        pszMainInstruction: pcwstr(&instruction),
        pszContent: pcwstr(&content),
        pszVerificationText: pcwstr(&verification),
        cButtons: 2,
        pButtons: buttons.as_ptr(),
        nDefaultButton: 101,
        ..Default::default()
    };
    let (mut pressed, mut checked) = (0, BOOL(0));
    unsafe {
        let _ = TaskDialogIndirect(&config, Some(&mut pressed), None, Some(&mut checked));
    }
    crate::updates::set_automatic(checked.as_bool());
    app.model.prefs.check_for_updates = checked.as_bool();
    app.model.prefs.save();
    if pressed == 100 {
        crate::updates::check(true);
    }
}

fn shell_item(path: &std::path::Path) -> Option<IShellItem> {
    let w = wide(path);
    unsafe { SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None).ok() }
}

fn result_path(dialog: &IFileDialog) -> Option<PathBuf> {
    unsafe {
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(name.to_string().ok()?);
        CoTaskMemFree(Some(name.0 as *const _));
        Some(path)
    }
}

/// Starts in the current photograph's folder with it selected.
pub fn open(app: &mut App) {
    let photos: String = RAW_EXTENSIONS.iter().chain(IMAGE_EXTENSIONS).map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";");
    let raw: String = RAW_EXTENSIONS.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";");
    let filters = [
        ("Photographs", photos),
        ("RAW photographs", raw),
        ("JPEG", "*.jpg;*.jpeg".into()),
        ("PNG", "*.png".into()),
        ("TIFF", "*.tif;*.tiff".into()),
        ("All files", "*.*".into()),
    ];
    let wide_filters: Vec<(Vec<u16>, Vec<u16>)> = filters.iter().map(|(n, p)| (wide(n), wide(p))).collect();
    let specs: Vec<COMDLG_FILTERSPEC> = wide_filters.iter().map(|(n, p)| COMDLG_FILTERSPEC { pszName: pcwstr(n), pszSpec: pcwstr(p) }).collect();
    let chosen = unsafe {
        (|| -> Option<PathBuf> {
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            dialog.SetClientGuid(&OPEN_GUID).ok()?;
            dialog.SetFileTypes(&specs).ok()?;
            let title = wide("Open Photograph");
            let _ = dialog.SetTitle(pcwstr(&title));
            let _ = dialog.SetOptions(FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM);
            if let Some(source) = &app.model.source {
                if let Some(folder) = source.parent().and_then(shell_item) {
                    let _ = dialog.SetFolder(&folder);
                }
                let name = wide(source.file_name().unwrap_or_default());
                let _ = dialog.SetFileName(pcwstr(&name));
            }
            dialog.Show(app.main).ok()?;
            result_path(&dialog.cast().ok()?)
        })()
    };
    if let Some(path) = chosen {
        app.model.open(&path);
    }
}

const ENGINE: u32 = 1;
const SIZE: u32 = 2;
const PIXELS: u32 = 3;
const PX: u32 = 4;
const MESSAGE: u32 = 5;
const DEPTH: u32 = 6;
const SPACE: u32 = 7;
const QUALITY: u32 = 8;
const QUALITY_RANGE_TEXT: u32 = 9;
const GROUP_ENGINE: u32 = 10;
const GROUP_SIZE: u32 = 11;
const GROUP_DEPTH: u32 = 12;
const GROUP_SPACE: u32 = 13;
const GROUP_QUALITY: u32 = 14;

/// The dialog's message, from what the model says about this save.
struct Messages {
    reference: String,
    fast: String,
    preview: String,
}

/// The encoding choices, shared with `save` to read them back. The colour
/// space is the one picked, which ProPhoto RGB at 8 bits only shows as
/// Adobe RGB, so 16 bits brings it back.
struct Encoding {
    format: Cell<OutputFormat>,
    chosen_space: Cell<usize>,
    /// The colour space the preview develops in.
    preview_space: String,
    png_depth: Cell<u32>,
    tiff_depth: Cell<u32>,
    /// Set while the controls are being brought in line, so their
    /// selection events don't count as choices.
    updating: Cell<bool>,
}

impl Encoding {
    fn depth(&self) -> u32 {
        match self.format.get() {
            OutputFormat::Jpeg => 8,
            OutputFormat::Tiff => self.tiff_depth.get(),
            OutputFormat::Png => self.png_depth.get(),
        }
    }
    fn settings(&self, customize: &IFileDialogCustomize) -> SaveSettings {
        let bit_depth = self.depth();
        SaveSettings { bit_depth, color_space: SaveSettings::effective_space(self.chosen_space.get(), bit_depth), jpeg_quality: read_quality(customize) }
    }
}

#[implement(IFileDialogEvents, IFileDialogControlEvents)]
struct SaveEvents {
    export: bool,
    messages: Messages,
    encoding: Rc<Encoding>,
    extras: String,
}

impl SaveEvents {
    /// Shows the format's bit depth (JPEG's fixed), ProPhoto RGB only at 16
    /// bits, and JPEG quality only for JPEG; then the message.
    fn sync(&self, customize: &IFileDialogCustomize) {
        let encoding = &self.encoding;
        let jpeg = encoding.format.get() == OutputFormat::Jpeg;
        let settings = encoding.settings(customize);
        encoding.updating.set(true);
        unsafe {
            let _ = customize.SetSelectedControlItem(DEPTH, if settings.bit_depth == 16 { 1 } else { 0 });
            let _ = customize.SetControlState(DEPTH, if jpeg { CDCS_VISIBLE } else { CDCS_ENABLEDVISIBLE });
            let _ = customize.SetControlItemState(SPACE, PROPHOTO as u32, if settings.bit_depth == 16 { CDCS_ENABLEDVISIBLE } else { CDCS_VISIBLE });
            let _ = customize.SetSelectedControlItem(SPACE, settings.color_space as u32);
            for id in [GROUP_QUALITY, QUALITY, QUALITY_RANGE_TEXT] {
                let _ = customize.SetControlState(id, if jpeg { CDCS_ENABLEDVISIBLE } else { CDCS_INACTIVE });
            }
        }
        encoding.updating.set(false);
        self.message(customize);
    }

    fn message(&self, customize: &IFileDialogCustomize) {
        unsafe {
            let engine = if self.export {
                Some(if customize.GetSelectedControlItem(ENGINE).unwrap_or(1) == 0 { ExportEngine::Reference } else { ExportEngine::Fast })
            } else {
                None
            };
            let size = read_size(customize);
            let lead = match engine {
                Some(ExportEngine::Reference) => &self.messages.reference,
                Some(ExportEngine::Fast) => &self.messages.fast,
                None => &self.messages.preview,
            };
            let summary = size.summary().map(|s| format!(" {s}.")).unwrap_or_default();
            let space = self.encoding.settings(customize).summary(self.export, &self.encoding.preview_space).map(|s| format!(" {s}.")).unwrap_or_default();
            let text = wide(format!("{lead}{}{summary}{space}", self.extras));
            let _ = customize.SetControlLabel(MESSAGE, pcwstr(&text));
            let _ = customize.SetControlState(PIXELS, if size.edge == Edge::Full { CDCS_VISIBLE } else { CDCS_ENABLEDVISIBLE });
        }
    }
}

fn read_quality(customize: &IFileDialogCustomize) -> u32 {
    unsafe {
        customize
            .GetEditBoxText(QUALITY)
            .ok()
            .map(|p| {
                let text = PWSTR(p).to_string().unwrap_or_default();
                CoTaskMemFree(Some(p as *const _));
                text
            })
            .and_then(|t| t.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok())
            .unwrap_or(95)
            .clamp(QUALITY_RANGE.0, QUALITY_RANGE.1)
    }
}

fn read_size(customize: &IFileDialogCustomize) -> ExportSize {
    unsafe {
        let edge = Edge::ALL[customize.GetSelectedControlItem(SIZE).unwrap_or(0).min(2) as usize];
        let pixels = customize
            .GetEditBoxText(PIXELS)
            .ok()
            .map(|p| {
                let text = PWSTR(p).to_string().unwrap_or_default();
                CoTaskMemFree(Some(p as *const _));
                text
            })
            .and_then(|t| t.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok())
            .unwrap_or(ExportSize::FULL.pixels)
            .clamp(PIXEL_RANGE.0, PIXEL_RANGE.1);
        ExportSize { edge, pixels }
    }
}

impl IFileDialogEvents_Impl for SaveEvents_Impl {
    fn OnFileOk(&self, _: Option<&IFileDialog>) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnFolderChanging(&self, _: Option<&IFileDialog>, _: Option<&IShellItem>) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnFolderChange(&self, _: Option<&IFileDialog>) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnSelectionChange(&self, _: Option<&IFileDialog>) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnShareViolation(&self, _: Option<&IFileDialog>, _: Option<&IShellItem>) -> windows::core::Result<FDE_SHAREVIOLATION_RESPONSE> {
        Ok(FDESVR_DEFAULT)
    }
    /// The extension follows the format.
    fn OnTypeChange(&self, dialog: Option<&IFileDialog>) -> windows::core::Result<()> {
        let Some(dialog) = dialog else { return Ok(()) };
        unsafe {
            let index = dialog.GetFileTypeIndex()?.clamp(1, 3);
            let format = OutputFormat::ALL[index as usize - 1];
            self.encoding.format.set(format);
            if let Ok(customize) = dialog.cast::<IFileDialogCustomize>() {
                self.sync(&customize);
            }
            let extension = wide(format.extension());
            dialog.SetDefaultExtension(pcwstr(&extension))?;
            if let Ok(name) = dialog.GetFileName() {
                let current = name.to_string().unwrap_or_default();
                CoTaskMemFree(Some(name.0 as *const _));
                let stem = std::path::Path::new(&current).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(current);
                if !stem.is_empty() {
                    let renamed = wide(format!("{stem}.{}", format.extension()));
                    let _ = dialog.SetFileName(pcwstr(&renamed));
                }
            }
        }
        Ok(())
    }
    fn OnOverwrite(&self, _: Option<&IFileDialog>, _: Option<&IShellItem>) -> windows::core::Result<FDE_OVERWRITE_RESPONSE> {
        Ok(FDEOR_DEFAULT)
    }
}

impl IFileDialogControlEvents_Impl for SaveEvents_Impl {
    fn OnItemSelected(&self, customize: Option<&IFileDialogCustomize>, control: u32, item: u32) -> windows::core::Result<()> {
        let Some(customize) = customize else { return Ok(()) };
        let encoding = &self.encoding;
        if encoding.updating.get() {
            return Ok(());
        }
        match control {
            DEPTH => {
                let depth = if item == 1 { 16 } else { 8 };
                match encoding.format.get() {
                    OutputFormat::Png => encoding.png_depth.set(depth),
                    OutputFormat::Tiff => encoding.tiff_depth.set(depth),
                    OutputFormat::Jpeg => {}
                }
                self.sync(customize);
            }
            SPACE => {
                encoding.chosen_space.set(item as usize);
                self.sync(customize);
            }
            _ => self.message(customize),
        }
        Ok(())
    }
    fn OnButtonClicked(&self, _: Option<&IFileDialogCustomize>, _: u32) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnCheckButtonToggled(&self, _: Option<&IFileDialogCustomize>, _: u32, _: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnControlActivating(&self, customize: Option<&IFileDialogCustomize>, _: u32) -> windows::core::Result<()> {
        if let Some(customize) = customize {
            self.message(customize);
        }
        Ok(())
    }
}

/// A separate LUT save dialog, available even without a photograph.
pub fn export_lut(app: &mut App) {
    if !app.model.can_export_lut() {
        return;
    }
    let state = &app.model.state;
    let target = if crate::json::boolean(&state.params["io"]["scan_film"]) { "direct_scan" } else { &state.paper };
    let filename = wide(format!("{}_{}.cube", state.film, target));
    let selected = unsafe {
        (|| -> windows::core::Result<PathBuf> {
            let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetClientGuid(&LUT_GUID)?;
            let title = wide("Export LUT");
            dialog.SetTitle(pcwstr(&title))?;
            let name = wide("3D LUT (.cube)");
            let pattern = wide("*.cube");
            dialog.SetFileTypes(&[COMDLG_FILTERSPEC { pszName: pcwstr(&name), pszSpec: pcwstr(&pattern) }])?;
            let extension = wide("cube");
            dialog.SetDefaultExtension(pcwstr(&extension))?;
            dialog.SetFileName(pcwstr(&filename))?;
            dialog.SetOptions(dialog.GetOptions()? | FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM | FOS_NOREADONLYRETURN)?;
            let customize: IFileDialogCustomize = dialog.cast()?;
            let message = wide("33 × 33 × 33 CUBE · sRGB input and output. Uses the current film and paper color settings. Auto exposure, print filter shifts, preflash, grain, halation, glare, diffusion, blur, sharpening, RAW development, and Post edits are excluded.");
            customize.AddText(MESSAGE, pcwstr(&message))?;
            dialog.Show(app.main)?;
            result_path(&dialog.cast()?).ok_or_else(windows::core::Error::from_win32)
        })()
    };
    if let Ok(path) = selected {
        app.model.export_lut(path);
    }
}

/// Save Preview or Export: the format in the type filter, the engine and
/// size beside it, and a message that follows the choices.
pub fn save(app: &mut App, export: bool) {
    let model = &app.model;
    if model.source.is_none() || model.cropping() || (export && !model.can_export()) || (!export && !model.can_save()) {
        return;
    }
    let prefs = &model.prefs;
    let format = OutputFormat::from_key(&prefs.output_format);
    let engine = ExportEngine::from_key(&prefs.export_engine);
    let size = ExportSize { edge: Edge::from_key(&prefs.export_edge), pixels: prefs.export_edge_pixels.clamp(PIXEL_RANGE.0, PIXEL_RANGE.1) };
    let lead = |engine: Option<ExportEngine>| crate::model::Model::save_lead(engine).to_string();
    let extras = model.save_extras();
    let depth = |d: u32| if d == 16 { 16 } else { 8 };
    let encoding = Rc::new(Encoding {
        format: Cell::new(format),
        chosen_space: Cell::new(COLOR_SPACES.iter().position(|s| *s == prefs.color_space).unwrap_or(0)),
        preview_space: model.preview_space().to_string(),
        png_depth: Cell::new(depth(prefs.png_bit_depth)),
        tiff_depth: Cell::new(depth(prefs.tiff_bit_depth)),
        updating: Cell::new(false),
    });
    let quality = prefs.jpeg_quality.clamp(QUALITY_RANGE.0, QUALITY_RANGE.1);
    let events: IFileDialogEvents = SaveEvents {
        export,
        messages: Messages { reference: lead(Some(ExportEngine::Reference)), fast: lead(Some(ExportEngine::Fast)), preview: lead(None) },
        encoding: encoding.clone(),
        extras,
    }
    .into();
    let name = model.default_name(format);
    let folder = prefs.save_folder().map(|p| p.to_path_buf());
    let main = app.main;
    let chosen = unsafe {
        (|| -> Option<(PathBuf, OutputFormat, ExportEngine, ExportSize, SaveSettings)> {
            let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            dialog.SetClientGuid(if export { &EXPORT_GUID } else { &PREVIEW_GUID }).ok()?;
            let names: Vec<(Vec<u16>, Vec<u16>)> = OutputFormat::ALL.iter().map(|f| (wide(f.title()), wide(f.pattern()))).collect();
            let specs: Vec<COMDLG_FILTERSPEC> = names.iter().map(|(n, p)| COMDLG_FILTERSPEC { pszName: pcwstr(n), pszSpec: pcwstr(p) }).collect();
            dialog.SetFileTypes(&specs).ok()?;
            dialog.SetFileTypeIndex(OutputFormat::ALL.iter().position(|f| *f == format).unwrap_or(0) as u32 + 1).ok()?;
            let extension = wide(format.extension());
            let _ = dialog.SetDefaultExtension(pcwstr(&extension));
            let title = wide(if export { "Export Photograph" } else { "Save Preview" });
            let _ = dialog.SetTitle(pcwstr(&title));
            let options = dialog.GetOptions().unwrap_or_default();
            let _ = dialog.SetOptions(options | FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM | FOS_NOREADONLYRETURN);
            if let Some(folder) = folder.as_deref().and_then(shell_item) {
                let _ = dialog.SetFolder(&folder);
            }
            let file = wide(&name);
            let _ = dialog.SetFileName(pcwstr(&file));
            let customize: IFileDialogCustomize = dialog.cast().ok()?;
            if export {
                let label = wide("Engine:");
                let _ = customize.StartVisualGroup(GROUP_ENGINE, pcwstr(&label));
                let _ = customize.AddComboBox(ENGINE);
                for (i, e) in ExportEngine::ALL.iter().enumerate() {
                    let t = wide(e.title());
                    let _ = customize.AddControlItem(ENGINE, i as u32, pcwstr(&t));
                }
                let _ = customize.SetSelectedControlItem(ENGINE, if engine == ExportEngine::Reference { 0 } else { 1 });
                let _ = customize.EndVisualGroup();
            }
            let label = wide("Size:");
            let _ = customize.StartVisualGroup(GROUP_SIZE, pcwstr(&label));
            let _ = customize.AddComboBox(SIZE);
            for (i, edge) in Edge::ALL.iter().enumerate() {
                let t = wide(edge.title());
                let _ = customize.AddControlItem(SIZE, i as u32, pcwstr(&t));
            }
            let _ = customize.SetSelectedControlItem(SIZE, Edge::ALL.iter().position(|e| *e == size.edge).unwrap_or(0) as u32);
            let pixels = wide(size.pixels.to_string());
            let _ = customize.AddEditBox(PIXELS, pcwstr(&pixels));
            let px = wide("px");
            let _ = customize.AddText(PX, pcwstr(&px));
            let _ = customize.EndVisualGroup();
            let label = wide("Bit depth:");
            let _ = customize.StartVisualGroup(GROUP_DEPTH, pcwstr(&label));
            let _ = customize.AddComboBox(DEPTH);
            for (i, title) in ["8-bit", "16-bit"].iter().enumerate() {
                let t = wide(title);
                let _ = customize.AddControlItem(DEPTH, i as u32, pcwstr(&t));
            }
            let _ = customize.EndVisualGroup();
            let label = wide("Colour space:");
            let _ = customize.StartVisualGroup(GROUP_SPACE, pcwstr(&label));
            let _ = customize.AddComboBox(SPACE);
            for (i, title) in COLOR_SPACES.iter().enumerate() {
                let t = wide(if i == PROPHOTO { "ProPhoto RGB (16-bit)" } else { title });
                let _ = customize.AddControlItem(SPACE, i as u32, pcwstr(&t));
            }
            let _ = customize.EndVisualGroup();
            let label = wide("JPEG quality:");
            let _ = customize.StartVisualGroup(GROUP_QUALITY, pcwstr(&label));
            let text = wide(quality.to_string());
            let _ = customize.AddEditBox(QUALITY, pcwstr(&text));
            let range = wide(format!("({}–{})", QUALITY_RANGE.0, QUALITY_RANGE.1));
            let _ = customize.AddText(QUALITY_RANGE_TEXT, pcwstr(&range));
            let _ = customize.EndVisualGroup();
            let empty = wide("");
            let _ = customize.AddText(MESSAGE, pcwstr(&empty));
            let cookie = dialog.Advise(&events).ok();
            // Set the encoding controls and the message before the dialog shows.
            if let Ok(inner) = events.cast::<IFileDialogEvents>() {
                let _ = inner.OnTypeChange(&dialog);
            }
            let shown = dialog.Show(main);
            if let Some(cookie) = cookie {
                let _ = dialog.Unadvise(cookie);
            }
            shown.ok()?;
            let path = result_path(&dialog.cast().ok()?)?;
            let index = dialog.GetFileTypeIndex().unwrap_or(1).clamp(1, 3);
            let chosen_engine = if export && customize.GetSelectedControlItem(ENGINE).unwrap_or(1) == 0 { ExportEngine::Reference } else { ExportEngine::Fast };
            encoding.format.set(OutputFormat::ALL[index as usize - 1]);
            Some((path, OutputFormat::ALL[index as usize - 1], chosen_engine, read_size(&customize), encoding.settings(&customize)))
        })()
    };
    let Some((path, format, chosen_engine, size, settings)) = chosen else { return };
    let prefs = &mut app.model.prefs;
    prefs.output_format = format.key().into();
    prefs.color_space = COLOR_SPACES[encoding.chosen_space.get()].into();
    prefs.png_bit_depth = encoding.png_depth.get();
    prefs.tiff_bit_depth = encoding.tiff_depth.get();
    prefs.jpeg_quality = settings.jpeg_quality;
    prefs.export_edge = size.edge.key().into();
    prefs.export_edge_pixels = size.pixels;
    prefs.last_save_folder = path.parent().map(|p| p.to_path_buf());
    if export {
        prefs.export_engine = chosen_engine.key().into();
    }
    prefs.save();
    if export {
        app.model.export(path, chosen_engine, size, settings);
    } else {
        app.model.save_preview(path, size, settings);
    }
    app.model.follow_preview_space();
}
