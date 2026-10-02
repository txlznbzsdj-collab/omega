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

use crate::format::{self, Style};
use crate::Engine;
use std::ptr;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateFontW, DeleteObject, GetStockObject, UpdateWindow, ANSI_FIXED_FONT, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, FF_MODERN, FIXED_PITCH, HFONT, OUT_DEFAULT_PRECIS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{EM_SCROLLCARET, EM_SETSEL};
// `SetFocus` lives under Input::KeyboardAndMouse and `UpdateWindow` under
// Graphics::Gdi in windows-sys, not with the rest of the window messages.
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_RETURN};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetParent, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, LoadCursorW, MessageBoxW,
    MoveWindow, PostQuitMessage, RegisterClassW, SendMessageW, SetWindowLongPtrW, SetWindowTextW,
    ShowWindow, TranslateMessage, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, EN_CHANGE,
    ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, GWLP_USERDATA, GWLP_WNDPROC,
    IDC_ARROW, MB_ICONERROR, MB_OK, MSG, SW_SHOW, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY,
    WM_ERASEBKGND, WM_GETFONT, WM_KEYDOWN, WM_NCCREATE, WM_SETFONT, WM_SIZE, WNDCLASSW, WNDPROC,
    WS_CHILD, WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

/// Identifier of the input edit control, and therefore of the notification code
/// that arrives in `WM_COMMAND` when the user presses Enter in it.
const ID_INPUT: i32 = 1;

/// The result area needs its own identifier purely so the two child controls can
/// be told apart; nothing is dispatched on it.
const ID_OUTPUT: i32 = 2;

/// Gaps and minimum sizes, in pixels, for the manual layout.
const MARGIN: i32 = 8;
const GAP: i32 = 6;
const MIN_INPUT_HEIGHT: i32 = 22;
const MIN_OUTPUT_HEIGHT: i32 = 80;

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
    engine: Engine,
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
            let state = Box::new(Window {
                input: ptr::null_mut(),
                output: ptr::null_mut(),
                font: ptr::null_mut(),
                font_owned: false,
                input_proc: 0,
                engine: Engine::new(),
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
                layout(&*state);
                focus_input(&*state);
                1
            }
        }
        // Every control fills its client area opaquely, so Windows' flicker-free
        // resizing would erase to the background first and flash white.
        WM_ERASEBKGND => 1,
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
            // An edit control reports progress through `EN_CHANGE`, not through
            // notification code 0. Live evaluation on every change means the
            // answer is already on screen when Enter is pressed, which also
            // covers the case where the window has no default button to give
            // the key to.
            if id == ID_INPUT && notification == EN_CHANGE {
                let state = state_of(window);
                if !state.is_null() {
                    evaluate(&mut *state);
                }
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
    Ok(())
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
        evaluate(&mut *state);
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

/// Splits the client area into the input strip, the result area, and the
/// margins between them. Recomputed on every `WM_SIZE` because the window is
/// resizable and no control is anchored automatically.
unsafe fn layout(state: &Window) {
    let mut client: RECT = std::mem::zeroed();
    if GetClientRect(window_of(state), &mut client) == 0 {
        return;
    }
    let client_width = client.right - client.left;
    let client_height = client.bottom - client.top;

    // A fixed number of text lines for the input keeps the result area's share
    // of the window predictable while the window is being resized.
    let content_height = (client_height - 2 * MARGIN - GAP).max(0);
    let input_height = (content_height / 6).clamp(MIN_INPUT_HEIGHT, 3 * MIN_INPUT_HEIGHT);
    let output_height = (content_height - input_height).max(MIN_OUTPUT_HEIGHT);
    let width = (client_width - 2 * MARGIN).max(1);

    MoveWindow(state.input, MARGIN, MARGIN, width, input_height, 1);
    MoveWindow(
        state.output,
        MARGIN,
        MARGIN + input_height + GAP,
        width,
        output_height,
        1,
    );
}

/// The window a state belongs to, read back from its input control.
unsafe fn window_of(state: &Window) -> HWND {
    GetParent(state.input)
}

/// Evaluates the expression in the input control and shows the outcome.
///
/// Errors are part of the result here, not an exception: a syntax error is what
/// the user is most often looking at, so it belongs in the same place the
/// answer would have appeared.
unsafe fn evaluate(state: &mut Window) {
    let expression = window_text(state.input);
    if expression.trim().is_empty() {
        return;
    }
    let outcome = match state.engine.eval(&expression) {
        Ok(value) => format::render(&value, Style::default()),
        Err(error) => format::Rendered {
            text: error.to_string(),
            note: None,
        },
    };
    state.last_output = match outcome.note {
        Some(note) => format!("{}\r\n{}", outcome.text, note),
        None => outcome.text,
    };
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
