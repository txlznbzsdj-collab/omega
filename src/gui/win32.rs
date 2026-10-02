//! A single-window Win32 front end for the engine.
//!
//! The window is a composite of child controls rather than a single canvas:
//! an `EDIT` holds the expression, a read-only `EDIT` holds the result. That
//! choice is what makes long answers practical — a hundred thousand digits are
//! clipped and scrolled by the edit control itself, so the front end never
//! measures or draws text, it only hands the engine's output to `SetWindowTextW`
//! and lets the control manage the expensive part.
//!
//! Evaluation happens synchronously in the window procedure. No window is
//! created or destroyed while a computation runs, so a long answer makes the
//! program unresponsive for as long as the engine takes; the alternative, a
//! worker thread, would add synchronization for a case the engine already keeps
//! fast.

use crate::format::{Grouping, Style};
use crate::gui::{edit, keypad, worker};
use std::ptr;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateFontW, DeleteObject, GetStockObject, UpdateWindow, ANSI_FIXED_FONT, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, FF_MODERN, FIXED_PITCH, HFONT, OUT_DEFAULT_PRECIS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{EM_GETSEL, EM_SCROLLCARET, EM_SETSEL};
// `SetFocus` lives under Input::KeyboardAndMouse and `UpdateWindow` under
// Graphics::Gdi in windows-sys, not with the rest of the window messages.
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_RETURN};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuItem, CreateMenu, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW, GetParent,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, LoadCursorW, MessageBoxW, MoveWindow,
    PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW, SetMenu, SetWindowLongPtrW,
    SetWindowTextW, ShowWindow, TranslateMessage, BS_PUSHBUTTON, CREATESTRUCTW, CS_HREDRAW,
    CS_VREDRAW, CW_USEDEFAULT, EN_CHANGE, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE,
    ES_READONLY, GWLP_USERDATA, GWLP_WNDPROC, HMENU, IDC_ARROW, MB_ICONERROR, MB_OK, MF_BYCOMMAND,
    MF_CHECKED, MF_POPUP, MF_STRING, MF_UNCHECKED, MSG, SW_SHOW, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_CREATE, WM_DESTROY, WM_ERASEBKGND, WM_GETFONT, WM_KEYDOWN, WM_NCCREATE, WM_SETFONT, WM_SIZE,
    WNDCLASSW, WNDPROC, WS_CHILD, WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};

/// Identifier of the input edit control, and therefore of the notification code
/// that arrives in `WM_COMMAND` when the user presses Enter in it.
const ID_INPUT: i32 = 1;

/// The result area needs its own identifier purely so the two child controls can
/// be told apart; nothing is dispatched on it.
const ID_OUTPUT: i32 = 2;

/// The most keys the pad may ever hold, used to size the command-id range. The
/// keypad module owns the actual list; this only has to be generous enough that
/// the range check stays valid.
const MAX_KEYS: usize = 64;

/// Posted by the worker when an answer is ready. `WM_APP` is the first value
/// Windows leaves to the application, so it cannot collide with a system
/// message.
const WM_APP_ANSWER: u32 = WM_APP + 1;

/// Command ids for the menu. Grouped away from the keypad range so a misread
/// command cannot be mistaken for a key press.
const ID_MENU_GROUP_NONE: i32 = 900;
const ID_MENU_GROUP_UNDERSCORE: i32 = 901;
const ID_MENU_GROUP_COMMA: i32 = 902;
const ID_MENU_GROUP_SPACE: i32 = 903;

/// Gaps and minimum sizes, in pixels, for the manual layout.
const MARGIN: i32 = 8;
const GAP: i32 = 6;
const MIN_INPUT_HEIGHT: i32 = 22;

/// The keypad is sized from its content: each column needs room for the widest
/// label, and neighbours are separated by `KEY_GAP` so the labels do not touch.
const MIN_KEY_WIDTH: i32 = 52;
const KEY_GAP: i32 = 4;
const MIN_KEYPAD_WIDTH: i32 = 200;
/// Rows stop growing past this, so a tall window gives the extra space to the
/// result area instead of producing oversized buttons.
const MAX_KEY_HEIGHT: i32 = 34;

/// A font that is installed on essentially every Windows system, used when
/// Consolas is not available. Both are fixed pitch, which results need: a
/// proportional digit string is unreadable at length.
const FALLBACK_FACE: &[u16] = &[
    b'C' as u16,
    b'o' as u16,
    b'u' as u16,
    b'r' as u16,
    b'i' as u16,
    b'e' as u16,
    b'r' as u16,
    b' ' as u16,
    b'N' as u16,
    b'e' as u16,
    b'w' as u16,
    0,
];

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A NUL-terminated UTF-16 copy of a compile-time literal.
///
/// `const` rather than `static` so each call materializes its own array, which
/// is what the `bool` behind `wide_opt` needs to pass a pointer out of this
/// frame.
const fn nul_terminated(units: &[u16]) -> ([u16; 32], usize) {
    let mut buffer = [0u16; 32];
    let mut index = 0;
    while index < units.len() {
        buffer[index] = units[index];
        index += 1;
    }
    (buffer, units.len())
}

/// Builds a font from the first typeface the system actually has.
///
/// `CreateFontW` succeeds even for a typeface that does not exist — it silently
/// substitutes — so the usual way to prefer one family over another is to ask
/// the system which names are installed and only then create the font.
fn create_monospace_font() -> HFONT {
    let installed = installed_face_names();
    let (units, length) = nul_terminated(FALLBACK_FACE);
    let fallback = &units[..length];
    let face = if installed.contains("Consolas") {
        "Consolas".to_string()
    } else if installed.contains("Courier New") {
        "Courier New".to_string()
    } else {
        // Nothing familiar is installed; the family request below still lands
        // on some fixed-pitch face.
        String::from_utf16_lossy(fallback)
    };
    let face: Vec<u16> = face.encode_utf16().chain(std::iter::once(0)).collect();
    let font = unsafe {
        CreateFontW(
            0,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            0,
            // `FF_MODERN` plus `FIXED_PITCH` is the request that matters: even
            // if the name is missing, the substitution stays monospaced.
            (FF_MODERN | FIXED_PITCH) as u32,
            face.as_ptr(),
        )
    };
    if font.is_null() {
        // A stock font is not the one we asked for, but it draws text, which
        // beats leaving the controls with the system default.
        unsafe { GetStockObject(ANSI_FIXED_FONT) as HFONT }
    } else {
        font
    }
}

/// The typeface names the system can actually produce.
///
/// `EnumFontFamiliesExW` would be the exact answer but needs a callback that
/// outlives this frame's borrows; the installed-file list is close enough,
/// since a face without an installed file cannot be selected either.
fn installed_face_names() -> String {
    const FONT_DIR: &str = r"C:\Windows\Fonts";
    std::fs::read_dir(FONT_DIR)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect::<Vec<String>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Window state reached from the window procedure through `GWLP_USERDATA`.
struct Window {
    input: HWND,
    output: HWND,
    font: HFONT,
    /// Whether `font` was created rather than borrowed from the stock objects,
    /// and so is ours to delete. `GetStockObject` handles must never be freed.
    font_owned: bool,
    /// The input control's original window procedure, restored by the subclass
    /// for every message it does not handle itself.
    input_proc: isize,
    /// Keypad buttons in grid order, so a command id can be turned back into a
    /// key without searching.
    buttons: Vec<HWND>,
    /// Menu handle, kept so the checked item can be updated when a setting
    /// changes.
    menu: HMENU,
    /// How results are arranged: significant digits and digit grouping.
    style: Style,
    /// Runs evaluations off this thread, so a slow one cannot freeze the
    /// window.
    worker: worker::Worker,
    /// The generation of the answer the window is waiting to display.
    expected: u64,
    /// The rendered result, kept so `WM_GETFONT`/rescaling would not have to
    /// recompute anything, and so the text handed to `SetWindowTextW` outlives
    /// the call.
    last_output: String,
}

/// Entry point of the graphical front end; returns the process exit code.
pub fn main() -> i32 {
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let class_name = wide("OmegaWindowClass");
        let window_title = wide("omega - arbitrary-precision calculator");

        let mut class: WNDCLASSW = std::mem::zeroed();
        class.style = CS_HREDRAW | CS_VREDRAW;
        class.lpfnWndProc = Some(wndproc);
        class.hInstance = instance;
        class.hCursor = LoadCursorW(ptr::null_mut(), IDC_ARROW);
        class.lpszClassName = class_name.as_ptr();
        if RegisterClassW(&class) == 0 {
            return fail("could not register the window class");
        }

        let window = CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            900,
            620,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null_mut(),
        );
        if window.is_null() {
            return fail("could not create the window");
        }

        ShowWindow(window, SW_SHOW);
        UpdateWindow(window);

        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        message.wParam as i32
    }
}

/// Reports a startup failure the only way a windowed program can, and returns
/// the exit code for it.
fn fail(what: &str) -> i32 {
    let text = wide(what);
    let caption = wide("omega");
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
    1
}

unsafe extern "system" fn wndproc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // `WM_NCCREATE` arrives before `WM_CREATE` and carries the `CREATESTRUCTW`
        // we were handed, which is the earliest point the state can be attached
        // to the window. The state is built here rather than in `WM_CREATE` so
        // that a failure later in creation still leaves something to clean up.
        WM_NCCREATE => {
            let create = lparam as *const CREATESTRUCTW;
            if create.is_null() {
                return 0;
            }
            // The handle crosses threads as an integer: `HWND` is a raw pointer
            // and so is not `Send`, but posting to it is safe from any thread.
            let target = window as isize;
            let state = Box::new(Window {
                input: ptr::null_mut(),
                output: ptr::null_mut(),
                font: ptr::null_mut(),
                font_owned: false,
                input_proc: 0,
                buttons: Vec::new(),
                menu: ptr::null_mut(),
                style: Style::default(),
                // The worker wakes the window by posting, which is safe from
                // another thread; touching the controls from there would not be.
                worker: worker::Worker::start(move || {
                    PostMessageW(target as HWND, WM_APP_ANSWER, 0, 0);
                }),
                expected: 0,
                last_output: String::new(),
            });
            SetWindowLongPtrW(window, GWLP_USERDATA, Box::into_raw(state) as isize);
            DefWindowProcW(window, message, wparam, lparam)
        }
        WM_CREATE => {
            let state = state_of(window);
            // A null state or a control that failed to be created cannot be
            // recovered from, and -1 tells Windows to abort window creation
            // rather than leave a half-built window on screen.
            if state.is_null() || create_controls(window, &mut *state).is_err() {
                -1
            } else {
                // The menu is attached before the first layout so the client
                // area already excludes it when the controls are measured.
                match create_menu(window) {
                    Ok(menu) => (*state).menu = menu,
                    Err(()) => return -1,
                }
                set_grouping(&mut *state, ID_MENU_GROUP_NONE);
                layout(&*state);
                focus_input(&*state);
                1
            }
        }
        // Every control fills its client area opaquely, so Windows' flicker-free
        // resizing would erase to the background first and flash white.
        WM_ERASEBKGND => 1,
        // Posted by the worker once an expression has been evaluated. The
        // window is idle by now, so writing the result here cannot block
        // anything the user is waiting on.
        WM_APP_ANSWER => {
            let state = state_of(window);
            if !state.is_null() {
                show_answer(&mut *state);
            }
            0
        }
        WM_SIZE => {
            let state = state_of(window);
            if !state.is_null() {
                layout(&*state);
            }
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xffff) as i32;
            let notification = ((wparam >> 16) & 0xffff) as u32;
            let state = state_of(window);
            if state.is_null() {
                return 0;
            }
            // A keypad button reports `BN_CLICKED`, which is 0, so unlike the
            // edit control it is identified by its id alone.
            if (keypad::ID_KEY_FIRST..keypad::ID_KEY_FIRST + MAX_KEYS as i32).contains(&id) {
                press(&mut *state, (id - keypad::ID_KEY_FIRST) as usize);
            } else if (ID_MENU_GROUP_NONE..=ID_MENU_GROUP_SPACE).contains(&id) {
                set_grouping(&mut *state, id);
            } else if id == ID_INPUT && notification == EN_CHANGE {
                // An edit control reports progress through `EN_CHANGE`, not
                // through notification code 0. Live evaluation on every change
                // means the answer is already on screen when Enter is pressed.
                request_evaluation(&mut *state);
            }
            0
        }
        WM_SETFONT | WM_GETFONT => {
            // The edit controls forward these to the parent; answering with our
            // own handle keeps them monospaced whichever direction they ask.
            let state = state_of(window);
            if state.is_null() {
                0
            } else {
                (*state).font as LRESULT
            }
        }
        WM_CLOSE => {
            DestroyWindow(window);
            0
        }
        WM_DESTROY => {
            let state = state_of(window);
            if !state.is_null() {
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                // Taking the box back out of the raw pointer is what releases
                // the engine and the cached result.
                let state = Box::from_raw(state);
                if state.font_owned && !state.font.is_null() {
                    DeleteObject(state.font as _);
                }
                drop(state);
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

/// The state attached to `window`, or null if it has already been released.
unsafe fn state_of(window: HWND) -> *mut Window {
    GetWindowLongPtrW(window, GWLP_USERDATA) as *mut Window
}

/// Builds the menu bar and attaches it to the window.
///
/// Built in code rather than loaded from a resource so the program stays a
/// single binary with no `.rc` step in the build.
unsafe fn create_menu(window: HWND) -> Result<HMENU, ()> {
    let bar = CreateMenu();
    if bar.is_null() {
        return Err(());
    }
    let grouping = CreatePopupMenu();
    if grouping.is_null() {
        DestroyMenu(bar);
        return Err(());
    }

    for (id, label) in [
        (ID_MENU_GROUP_NONE, "None"),
        (ID_MENU_GROUP_UNDERSCORE, "Underscore  1_000_000"),
        (ID_MENU_GROUP_COMMA, "Comma  1,000,000"),
        (ID_MENU_GROUP_SPACE, "Space  1 000 000"),
    ] {
        let text = wide(label);
        AppendMenuW(grouping, MF_STRING, id as usize, text.as_ptr());
    }

    let caption = wide("&Digits");
    AppendMenuW(
        bar,
        MF_POPUP | MF_STRING,
        grouping as usize,
        caption.as_ptr(),
    );

    if SetMenu(window, bar) == 0 {
        DestroyMenu(bar);
        return Err(());
    }
    Ok(bar)
}

/// Applies a grouping choice from the menu and re-renders the current answer.
unsafe fn set_grouping(state: &mut Window, id: i32) {
    state.style.grouping = match id {
        ID_MENU_GROUP_UNDERSCORE => Grouping::Underscore,
        ID_MENU_GROUP_COMMA => Grouping::Comma,
        ID_MENU_GROUP_SPACE => Grouping::Space,
        _ => Grouping::None,
    };
    for candidate in [
        ID_MENU_GROUP_NONE,
        ID_MENU_GROUP_UNDERSCORE,
        ID_MENU_GROUP_COMMA,
        ID_MENU_GROUP_SPACE,
    ] {
        let flag = if candidate == id {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };
        CheckMenuItem(state.menu, candidate as u32, MF_BYCOMMAND | flag);
    }
    // Re-rendered rather than recomputed: the worker re-evaluates the same
    // expression, but the engine's cost is in the arithmetic, and the style
    // only affects how the finished value is written out.
    request_evaluation(state);
}

/// Creates the two edit controls and applies the font to both.
unsafe fn create_controls(window: HWND, state: &mut Window) -> Result<(), ()> {
    let instance = GetModuleHandleW(ptr::null());
    let edit = wide("EDIT");

    let input = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        edit.as_ptr(),
        ptr::null(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        0,
        0,
        0,
        0,
        window,
        ID_INPUT as _,
        instance,
        ptr::null_mut(),
    );
    if input.is_null() {
        return Err(());
    }

    // `WS_VSCROLL` with `ES_MULTILINE` and `ES_READONLY` is the combination that
    // gives a scrollable result view for free; `WS_HSCROLL` is deliberately
    // absent so a long digit string wraps instead of disappearing off the right
    // edge. The vertical scrollbar is sized by the control whenever the text is
    // set, so a hundred thousand digits need no help from us.
    let output = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        edit.as_ptr(),
        ptr::null(),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WS_VSCROLL
            // The edit-control styles are `i32` in windows-sys while the window
            // styles are `u32`; the cast is the width, not the value.
            | ES_MULTILINE as u32
            | ES_READONLY as u32
            | ES_AUTOVSCROLL as u32,
        0,
        0,
        0,
        0,
        window,
        ID_OUTPUT as _,
        instance,
        ptr::null_mut(),
    );
    if output.is_null() {
        return Err(());
    }

    let previous = SetWindowLongPtrW(input, GWLP_WNDPROC, input_proc as *const () as isize);
    if previous == 0 {
        // Without the subclass Enter would do nothing, so treating this as a
        // failed creation is better than shipping a window that cannot compute.
        return Err(());
    }
    state.input_proc = previous;

    state.input = input;
    state.output = output;
    state.font = create_monospace_font();
    state.font_owned =
        !state.font.is_null() && state.font != unsafe { GetStockObject(ANSI_FIXED_FONT) as HFONT };
    // The handles travel through `WPARAM`, a signed pointer-width integer: a
    // null handle is the integer 0, not a cast null pointer.
    SendMessageW(input, WM_SETFONT, state.font as usize as WPARAM, 1);
    SendMessageW(output, WM_SETFONT, state.font as usize as WPARAM, 1);
    create_keypad(window, state)
}

/// Creates one button per keypad key and applies the font to all of them.
unsafe fn create_keypad(window: HWND, state: &mut Window) -> Result<(), ()> {
    let instance = GetModuleHandleW(ptr::null());
    let button = wide("BUTTON");

    for (index, key) in keypad::keys().enumerate() {
        let label = wide(key.label);
        let control = CreateWindowExW(
            0,
            button.as_ptr(),
            label.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            0,
            0,
            0,
            0,
            window,
            (keypad::ID_KEY_FIRST + index as i32) as _,
            instance,
            ptr::null_mut(),
        );
        if control.is_null() {
            return Err(());
        }
        SendMessageW(control, WM_SETFONT, state.font as usize as WPARAM, 1);
        state.buttons.push(control);
    }
    Ok(())
}

/// Handles a keypad press: edits the expression, or runs it for `=`.
unsafe fn press(state: &mut Window, index: usize) {
    let key = match keypad::key_at(index) {
        Some(key) => key,
        None => return,
    };
    match key.insert {
        keypad::ACTION_CLEAR => {
            SetWindowTextW(state.input, wide("").as_ptr());
            apply_input(state, "", 0);
            request_evaluation(state);
            return;
        }
        keypad::ACTION_BACKSPACE => {
            let (text, caret, end) = input_state(state.input);
            let result = edit::backspace(&text, caret, end);
            apply_input(state, &result.text, result.caret);
            request_evaluation(state);
            return;
        }
        keypad::ACTION_EVALUATE => {
            request_evaluation(state);
            return;
        }
        _ => {}
    }

    let (text, caret, end) = input_state(state.input);
    // Typing over a selection replaces it, the way an edit control would.
    let (text, caret) = if end != caret {
        let cleared = edit::backspace(&text, caret, end);
        (cleared.text, cleared.caret)
    } else {
        (text, caret)
    };
    let result = edit::insert(&text, caret, key.insert, key.caret);
    apply_input(state, &result.text, result.caret);
    // Asked for directly rather than left to the `EN_CHANGE` the text change
    // produces: that notification is delivered by `SendMessage`, so it arrives
    // re-entrantly in the middle of this function and can be missed when the
    // window is already busy.
    request_evaluation(state);
}

/// Replaces the input text and puts the caret where the edit left it.
unsafe fn apply_input(state: &mut Window, text: &str, caret: edit::Caret) {
    SetWindowTextW(state.input, wide(text).as_ptr());
    // `SetWindowTextW` resets the selection to the start, so the caret has to
    // be restored afterwards or every key press would jump to the front.
    SendMessageW(state.input, EM_SETSEL, caret, caret as isize);
    focus_input(state);
}

/// The input's text and both ends of its selection.
unsafe fn input_state(input: HWND) -> (String, edit::Caret, edit::Caret) {
    let text = window_text(input);
    let mut start = 0u32;
    let mut end = 0u32;
    SendMessageW(
        input,
        EM_GETSEL,
        &mut start as *mut u32 as WPARAM,
        &mut end as *mut u32 as LPARAM,
    );
    let length = text.chars().count() as u32;
    // A control with no selection reports both ends as `0xffffffff` style
    // sentinels; clamping keeps the caret arithmetic in range either way.
    (
        text,
        (start.min(length)) as usize,
        (end.min(length)) as usize,
    )
}

/// Window procedure for the input box.
///
/// A single-line edit control swallows `VK_RETURN` — it never reaches the
/// parent as a command, so Enter cannot be handled from `WM_COMMAND`. Taking
/// over the control's own procedure is the only way to see the key.
unsafe extern "system" fn input_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state = state_of(GetParent(window));
    if message == WM_KEYDOWN && wparam as u16 == VK_RETURN && !state.is_null() {
        request_evaluation(&mut *state);
        return 0;
    }
    // `state.input_proc` is whatever the control had before, read back out of
    // the integer it was stored as. Chaining to it rather than to
    // `DefWindowProcW` keeps the edit control's own editing behaviour.
    let previous = if state.is_null() {
        None
    } else {
        std::mem::transmute::<isize, WNDPROC>((*state).input_proc)
    };
    match previous {
        Some(procedure) => procedure(window, message, wparam, lparam),
        None => DefWindowProcW(window, message, wparam, lparam),
    }
}

/// Splits the client area into the input strip, the result area, the keypad,
/// and the margins between them. Recomputed on every `WM_SIZE` because the
/// window is resizable and no control is anchored automatically.
unsafe fn layout(state: &Window) {
    let mut client: RECT = std::mem::zeroed();
    if GetClientRect(window_of(state), &mut client) == 0 {
        return;
    }
    let client_width = client.right - client.left;
    let client_height = client.bottom - client.top;

    // The keypad is sized from what it holds rather than as a fraction of the
    // window: eight columns each need enough room for the longest label, or
    // `nthroot` is clipped to `hro` and the last column falls off the edge.
    let pad_width = (keypad::columns() as i32 * MIN_KEY_WIDTH
        + (keypad::columns() as i32 - 1) * KEY_GAP)
        .min((client_width / 2).max(MIN_KEYPAD_WIDTH));
    let text_width = (client_width - 2 * MARGIN - GAP - pad_width).max(1);
    let pad_left = MARGIN + text_width + GAP;

    let content_height = (client_height - 2 * MARGIN - GAP).max(0);
    let input_height = (content_height / 6).clamp(MIN_INPUT_HEIGHT, 3 * MIN_INPUT_HEIGHT);
    MoveWindow(state.input, MARGIN, MARGIN, text_width, input_height, 1);

    // The result area and the keypad start on the same line. The result area
    // runs to the bottom of the window, so the space beneath the compact keypad
    // is used rather than left blank, while the buttons stop at a height that
    // still looks like a keypad.
    let pad_top = MARGIN + input_height + GAP;
    let band_height = (client_height - MARGIN - pad_top).max(1);
    let pad_height = band_height.min(keypad::rows() as i32 * MAX_KEY_HEIGHT);

    MoveWindow(state.output, MARGIN, pad_top, text_width, band_height, 1);

    for (index, button) in state.buttons.iter().enumerate() {
        let (x, y, width, height) = keypad::cell(index, pad_width, pad_height, KEY_GAP);
        MoveWindow(*button, pad_left + x, pad_top + y, width, height, 1);
    }
}

/// The window a state belongs to, read back from its input control.
unsafe fn window_of(state: &Window) -> HWND {
    GetParent(state.input)
}

/// Hands the expression to the worker and remembers which answer to expect.
///
/// Nothing is computed here: rendering a large result takes seconds, and doing
/// it on this thread would stop the window from painting or closing. The answer
/// arrives later as `WM_APP_ANSWER`.
unsafe fn request_evaluation(state: &mut Window) {
    let expression = window_text(state.input);
    if expression.trim().is_empty() {
        // An emptied box should clear the result rather than leave the previous
        // answer sitting under an expression that no longer produces it.
        state.expected = 0;
        state.last_output.clear();
        SetWindowTextW(state.output, wide("").as_ptr());
        return;
    }
    state.expected = state.worker.submit(expression, state.style);
}

/// Shows the newest finished answer.
///
/// `take_latest` drains everything queued, so an answer that a later keystroke
/// has already overtaken is dropped instead of being briefly displayed. No
/// generation check is needed: the queue is ordered, so the last answer out is
/// the newest one.
unsafe fn show_answer(state: &mut Window) {
    let answer = match state.worker.take_latest() {
        Some(answer) => answer,
        None => return,
    };
    state.last_output = answer.text;
    let text = wide(&state.last_output);
    SetWindowTextW(state.output, text.as_ptr());
    // A fresh result should be read from its first line, not from wherever the
    // previous one was scrolled to.
    SendMessageW(
        state.output,
        EM_SETSEL,
        0,
        state.last_output.encode_utf16().count() as isize,
    );
    SendMessageW(state.output, EM_SCROLLCARET, 0, 0);
}

/// The current text of a control, or an empty string if it cannot be read.
unsafe fn window_text(control: HWND) -> String {
    let length = GetWindowTextLengthW(control);
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let read = GetWindowTextW(control, buffer.as_mut_ptr(), buffer.len() as i32);
    if read <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..read as usize])
}

/// Gives the input control the keyboard focus so the window is usable the
/// moment it appears.
fn focus_input(state: &Window) {
    unsafe {
        SetFocus(state.input);
    }
}
