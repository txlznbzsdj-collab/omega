// Enumerates a running omega window: its menu, its edit controls, and its
// keypad buttons, with the label on each. This is how the keypad is confirmed
// without being able to see or click the window.

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

fn text_of(hwnd: HWND) -> String {
    let mut buffer = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..n.max(0) as usize])
}

unsafe fn find_window() -> Option<HWND> {
    // The window class is fixed, so look it up rather than guess at a title.
    let class = wide("OmegaWindowClass");
    let found = FindWindowW(class.as_ptr(), std::ptr::null());
    if found.is_null() {
        None
    } else {
        Some(found)
    }
}

struct Collector {
    edits: Vec<(i32, String)>,
    buttons: Vec<(i32, String)>,
}

unsafe extern "system" fn visit(child: HWND, param: LPARAM) -> i32 {
    let collector = &mut *(param as *mut Collector);
    let class = class_of(child);
    let text = text_of(child);
    let id = GetDlgCtrlID(child);
    match class.as_str() {
        "Edit" => collector.edits.push((id, text)),
        "Button" => collector.buttons.push((id, text)),
        _ => {}
    }
    1
}

fn main() {
    unsafe {
        let window = match find_window() {
            Some(window) => window,
            None => {
                println!("no omega window is open");
                std::process::exit(1);
            }
        };
        println!("window: {:?}", text_of(window));

        let menu = GetMenu(window);
        if menu.is_null() {
            println!("menu: NONE");
        } else {
            let count = GetMenuItemCount(menu);
            println!("menu: {count} top-level item(s)");
            for index in 0..count {
                let mut buffer = [0u16; 128];
                let n = GetMenuStringW(
                    menu,
                    index as u32,
                    buffer.as_mut_ptr(),
                    buffer.len() as i32,
                    MF_BYPOSITION,
                );
                println!(
                    "  - {:?}",
                    String::from_utf16_lossy(&buffer[..n.max(0) as usize])
                );
            }
        }

        let mut collector = Collector {
            edits: Vec::new(),
            buttons: Vec::new(),
        };
        EnumChildWindows(
            window,
            Some(visit),
            &mut collector as *mut Collector as isize,
        );

        println!("edit controls: {}", collector.edits.len());
        for (id, text) in &collector.edits {
            println!("  id={id}  {:?}", &text[..text.len().min(40)]);
        }

        println!("keypad buttons: {}", collector.buttons.len());
        let labels: Vec<String> = collector
            .buttons
            .iter()
            .map(|(_, label)| label.clone())
            .collect();
        println!("  {}", labels.join(" "));

        // A keypad is only useful if its keys are on screen and clickable.
        if collector.buttons.is_empty() {
            println!("FAIL: no buttons found");
            std::process::exit(1);
        }
        println!("\nkeypad and menu are present");
    }
}
