// Measures how much text a Win32 multiline EDIT control actually accepts, so
// the window's result area is sized against a measured limit rather than a
// guessed one.
//
// Creates the same control style the GUI uses, hands it increasing amounts of
// text through `SetWindowTextW`, and reads back what the control kept.

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

unsafe extern "system" fn wndproc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(window, message, wparam, lparam)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn main() {
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = wide("OmegaLimitProbe");
        let mut class: WNDCLASSW = std::mem::zeroed();
        class.lpfnWndProc = Some(wndproc);
        class.hInstance = instance;
        class.lpszClassName = class_name.as_ptr();
        if RegisterClassW(&class) == 0 {
            eprintln!("RegisterClassW failed");
            std::process::exit(1);
        }

        let parent = CreateWindowExW(
            0,
            class_name.as_ptr(),
            wide("probe").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            600,
            400,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut(),
        );
        if parent.is_null() {
            eprintln!("CreateWindowExW(parent) failed");
            std::process::exit(1);
        }

        // Exactly the style the GUI gives its result area.
        let edit_class = wide("EDIT");
        let edit = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            edit_class.as_ptr(),
            std::ptr::null(),
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WS_VSCROLL
                | ES_MULTILINE as u32
                | ES_READONLY as u32
                | ES_AUTOVSCROLL as u32,
            0,
            0,
            400,
            300,
            parent,
            2 as _,
            instance,
            std::ptr::null_mut(),
        );
        if edit.is_null() {
            eprintln!("CreateWindowExW(edit) failed");
            std::process::exit(1);
        }

        for size in [
            1_000usize, 30_000, 32_767, 32_768, 100_000, 301_030, 1_000_000,
        ] {
            let payload = "7".repeat(size);
            let text = wide(&payload);
            SetWindowTextW(edit, text.as_ptr());
            let kept = GetWindowTextLengthW(edit);
            let verdict = if kept as usize == size {
                "accepted in full"
            } else {
                "TRUNCATED"
            };
            println!("{size:>9} chars sent -> {kept:>9} kept   {verdict}");
        }

        DestroyWindow(parent);
    }
}
