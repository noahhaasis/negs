//! The title bar's view switch (`ViewSwitch`, ticket 46): Grid · Photo ·
//! Compare in Newsreader 17 italic at the band's right, before the caption
//! buttons, whenever a photograph is open. The one shown is `paper` over
//! the pencil, the others `dim`. Compare acts with exactly two photographs
//! selected and is faint otherwise, its tooltip saying why; Grid and Photo
//! leave compare for them. The View menu keeps its items.
//!
//! The words are radio buttons drawn by `ink` (`Role::ViewWord`), so screen
//! readers hear which is shown. They're made the first time a photograph
//! opens.

use std::cell::Cell;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{HFONT, InvalidateRect};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use super::d2d::TextStyle;
use super::draw::{self, Role};
use super::{App, id, ink};

/// The words (`Theme.serif(17, italic: true)`).
pub const WORD: TextStyle = TextStyle::serif(17.0, 400).italic();
/// Between the words, after the last, and the switch's height, in DIPs.
const GAP: f64 = 16.0;
const TRAILING: f64 = 18.0;
const HEIGHT: f64 = 28.0;
/// Between the file name and the switch.
pub const NAME_GAP: f64 = 16.0;

const WORDS: [(u16, &str); 3] = [(id::VIEW_GRID, "Grid"), (id::VIEW_PHOTO, "Photo"), (id::VIEW_COMPARE, "Compare")];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Grid,
    Photo,
    Compare,
}

thread_local! {
    /// The three words, once made.
    static BUTTONS: Cell<[isize; 3]> = const { Cell::new([0; 3]) };
    static SHOWN: Cell<bool> = const { Cell::new(false) };
}

fn buttons() -> Option<[HWND; 3]> {
    let b = BUTTONS.with(Cell::get);
    (b[0] != 0).then(|| b.map(|h| HWND(h as *mut _)))
}

pub fn place(app: &App) -> Place {
    if super::grid::shown(app) {
        Place::Grid
    } else if app.model.comparison().is_some() {
        Place::Compare
    } else {
        Place::Photo
    }
}

/// What each word says under the pointer, and whether it acts.
fn words(app: &App, place: Place) -> [(bool, String); 3] {
    let model = &app.model;
    let comparing = place == Place::Compare;
    let grid = (
        model.shelf.library.is_some() && !model.cropping(),
        if comparing { "Stop comparing and show all photographs (G)" } else { "All Photographs (G)" }.to_string(),
    );
    let photo = (
        place != Place::Grid || model.shelf.selection.current().is_some(),
        if comparing { "Stop comparing and edit the photograph" } else { "Edit Photograph (E)" }.to_string(),
    );
    let (can, help) = model.compare_word();
    let compare = if comparing { (true, "Comparing two photographs (C to leave)".to_string()) } else { (can, help) };
    [grid, photo, compare]
}

/// Shows, marks and enables the words; made the first time a photograph
/// is open. Moves the band's name when the switch comes or goes.
pub fn sync(app: &mut App) {
    let shown = app.model.source.is_some();
    if shown && buttons().is_none() {
        let mut made = [0isize; 3];
        for (i, (command, title)) in WORDS.into_iter().enumerate() {
            let group = if i == 0 { WS_GROUP } else { WINDOW_STYLE(0) };
            let style = WS_CHILD | WS_CLIPSIBLINGS | group | WINDOW_STYLE((BS_RADIOBUTTON | BS_PUSHLIKE) as u32);
            let word = super::create(w!("BUTTON"), title, style, WINDOW_EX_STYLE(0), app.main, command, HFONT::default());
            draw::set_role(word, Role::ViewWord);
            draw::track_hover(word);
            made[i] = word.0 as isize;
        }
        BUTTONS.with(|b| b.set(made));
    }
    let Some(words_) = buttons() else { return };
    let place = place(app);
    let marks = [Place::Grid, Place::Photo, Place::Compare];
    for ((word, (enabled, help)), mark) in words_.into_iter().zip(words(app, place)).zip(marks) {
        unsafe {
            let on = (mark == place) as usize;
            if SendMessageW(word, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 as usize != on {
                SendMessageW(word, BM_SETCHECK, WPARAM(on), LPARAM(0));
                let _ = InvalidateRect(word, None, false);
            }
            if IsWindowEnabled(word).as_bool() != enabled {
                let _ = EnableWindow(word, enabled);
            }
        }
        // A faint word still says why.
        super::dialogs::tooltip(word, &help);
    }
    if SHOWN.with(|s| s.replace(shown)) != shown {
        for word in words_ {
            unsafe {
                let _ = ShowWindow(word, if shown { SW_SHOWNA } else { SW_HIDE });
            }
        }
        app.layout();
    }
}

/// Places the switch ending at `right` (pixels) in the band of `height`;
/// returns where it starts, or `right` while it's hidden.
pub fn layout(main: HWND, right: i32, height: i32) -> i32 {
    let Some(words_) = buttons().filter(|_| SHOWN.with(Cell::get)) else { return right };
    let s = draw::dpi_of(main) as f64;
    let px = |dip: f64| (dip * s).round() as i32;
    let margin = ink::MARGIN;
    let widths: Vec<f64> = WORDS.iter().map(|(_, title)| ink::width(title, &WORD) as f64 + 2.0 * margin).collect();
    let total: f64 = widths.iter().sum::<f64>() - 2.0 * margin * 3.0 + GAP * 2.0;
    let mut x = right as f64 / s - TRAILING - total - margin;
    let top = (height - px(HEIGHT)) / 2;
    let start = px(x + margin);
    for (word, width) in words_.into_iter().zip(widths) {
        unsafe {
            let _ = MoveWindow(word, px(x), top, px(width), px(HEIGHT), true);
        }
        x += width - 2.0 * margin + GAP;
    }
    start
}

/// The words' custom draw, from the main window's `WM_NOTIFY`.
pub fn notify(lparam: LPARAM) -> Option<LRESULT> {
    let from = unsafe { (*(lparam.0 as *const NMHDR)).hwndFrom };
    buttons()?.contains(&from).then(|| draw::custom_draw(lparam)).flatten()
}

/// A word's click: Grid and Photo leave compare for them; Compare
/// compares the two selected.
pub fn act(app: &mut App, command: u16) {
    let place = place(app);
    match command {
        id::VIEW_GRID => match place {
            Place::Compare => {
                app.model.leave_compare();
                super::grid::open(app);
            }
            Place::Photo => super::grid::open(app),
            Place::Grid => {}
        },
        id::VIEW_PHOTO => match place {
            Place::Compare => app.model.leave_compare(),
            Place::Grid => super::grid::close(app, true),
            Place::Photo => {}
        },
        id::VIEW_COMPARE if place != Place::Compare => super::compare::toggle(app),
        _ => {}
    }
}
