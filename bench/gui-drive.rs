// Drives the running window the way a person does: clicks real keypad buttons
// and checks that the result appears.
//
// This matters because `SetWindowTextW` from another process does not deliver
// `EN_CHANGE` to the parent across a process boundary, so a test that types by
// setting the text cannot tell a working window from a broken one. Clicking a
// button posts a command the window is guaranteed to receive.

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn class_of(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..n.max(0) as usize])
}

/// Reads a control's text from another process.
///
/// `GetWindowTextW` cannot read a control that belongs to a different process —
/// it deliberately refuses, to avoid hanging on an unresponsive owner — and
/// returns an empty string instead. `WM_GETTEXT` is delivered to the control
/// itself and works across the boundary, which is what makes this check
/// meaningful rather than always seeing "".
unsafe fn control_text(hwnd: HWND) -> String {
    let length = SendMessageW(hwnd, WM_GETTEXTLENGTH, 0, 0) as usize;
    if length == 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length + 1];
    let read = SendMessageW(hwnd, WM_GETTEXT, buffer.len(), buffer.as_mut_ptr() as isize);
    String::from_utf16_lossy(&buffer[..(read as usize).min(length)])
}

struct Found {
    input: HWND,
    output: HWND,
    buttons: Vec<(String, HWND)>,
}

unsafe extern "system" fn visit(child: HWND, param: LPARAM) -> i32 {
    let found = &mut *(param as *mut Found);
    match class_of(child).as_str() {
        "Edit" => {
            if found.input.is_null() {
                found.input = child;
            } else if found.output.is_null() {
                found.output = child;
            }
        }
        "Button" => found.buttons.push((control_text(child), child)),
        _ => {}
    }
    1
}

fn click(found: &Found, label: &str) -> bool {
    match found.buttons.iter().find(|(text, _)| text == label) {
        Some((_, hwnd)) => {
            unsafe {
                // BM_CLICK is delivered through the target's own message queue,
                // exactly like a real mouse press.
                SendMessageW(*hwnd, BM_CLICK, 0, 0);
            }
            true
        }
        None => false,
    }
}

fn main() {
    unsafe {
        let class = wide("OmegaWindowClass");
        let window = FindWindowW(class.as_ptr(), std::ptr::null());
        if window.is_null() {
            println!("no omega window is open");
            std::process::exit(1);
        }

        let mut found = Found {
            input: std::ptr::null_mut(),
            output: std::ptr::null_mut(),
            buttons: Vec::new(),
        };
        EnumChildWindows(window, Some(visit), &mut found as *mut Found as isize);
        if found.input.is_null() || found.output.is_null() {
            println!("could not find the edit controls");
            std::process::exit(1);
        }

        // Clear anything left from a previous run.
        click(&found, "C");

        let sequence = ["2", "^", "6", "4"];
        for label in sequence {
            if !click(&found, label) {
                println!("no button labelled {label:?}");
                std::process::exit(1);
            }
            std::thread::sleep(std::time::Duration::from_millis(120));
        }

        println!("typed on the keypad: {:?}", control_text(found.input));

        // The answer is computed on a worker thread, so give it a moment.
        let expected = "18446744073709551616";
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let shown = control_text(found.output);
            if shown.contains(expected) {
                println!("result pane shows: {}", &shown[..shown.len().min(40)]);
                println!("\nPASS: a keypad click produced the right answer");
                return;
            }
        }

        let shown = control_text(found.output);
        println!("result pane is still empty or wrong: {shown:?}");
        println!("\nFAIL: the answer never appeared");
        std::process::exit(1);
    }
}
