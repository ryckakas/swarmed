//! The menu bar: the system's own items, and a few of Swarmed's, which the window is told about.

use tauri::menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder, Submenu, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Runtime};

/// What the window hears when one of Swarmed's own items is chosen, with the item's id. What it
/// shows for each is up to the window (`menuChosen` in `ui/src/store.ts`): a new item of
/// Swarmed's gets its id in both.
pub(crate) const MENU_EVENT: &str = "menu";

const ABOUT: &str = "about";
const SETTINGS: &str = "settings";
const SHORTCUTS: &str = "shortcuts";

/// The menu bar, as a Mac app has it.
pub(crate) fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    MenuBuilder::new(app)
        .items(&[
            &app_menu(app)?,
            &edit_menu(app)?,
            &view_menu(app)?,
            &window_menu(app)?,
            &help_menu(app)?,
        ])
        .build()
}

/// Tells the window which of Swarmed's own items was chosen. The system's act by themselves.
pub(crate) fn chosen<R: Runtime>(app: &AppHandle<R>, event: &MenuEvent) {
    let id = event.id().as_ref();
    if [ABOUT, SETTINGS, SHORTCUTS].contains(&id) {
        app.emit(MENU_EVENT, id).ok();
    }
}

/// The menu named after the app. Its About is Swarmed's own view, not the system's panel.
fn app_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Submenu<R>> {
    let about = MenuItemBuilder::with_id(ABOUT, "About Swarmed").build(app)?;
    let settings = MenuItemBuilder::with_id(SETTINGS, "Settings…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    SubmenuBuilder::new(app, "Swarmed")
        .item(&about)
        .separator()
        .item(&settings)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()
}

/// On a Mac, undo, copy and paste reach a text field through these items: without them, the
/// keys do nothing in the window.
fn edit_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Submenu<R>> {
    SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()
}

fn view_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Submenu<R>> {
    SubmenuBuilder::new(app, "View").fullscreen().build()
}

fn window_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Submenu<R>> {
    SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .close_window()
        .build()
}

fn help_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Submenu<R>> {
    let shortcuts = MenuItemBuilder::with_id(SHORTCUTS, "Keyboard Shortcuts")
        .accelerator("CmdOrCtrl+/")
        .build(app)?;
    SubmenuBuilder::new(app, "Help").item(&shortcuts).build()
}
