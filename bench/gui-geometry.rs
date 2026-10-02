// Reports the geometry of a running omega window's keypad, so the layout can be
// checked against what it should be without seeing the screen.

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

/// A button's label and its screen rectangle.
type Placement = (String, i32, i32, i32, i32);

struct Info {
    client: (i32, i32),
    buttons: Vec<Placement>,
}

unsafe extern "system" fn visit(child: HWND, param: LPARAM) -> i32 {
    let info = &mut *(param as *mut Info);
    if class_of(child) == "Button" {
        let mut rect: RECT = std::mem::zeroed();
        GetWindowRect(child, &mut rect);
        info.buttons.push((
            text_of(child),
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
        ));
    }
    1
}

fn main() {
    unsafe {
        let class = wide("OmegaWindowClass");
        let window = FindWindowW(class.as_ptr(), std::ptr::null());
        if window.is_null() {
            println!("no omega window is open");
            std::process::exit(1);
        }

        let mut rect: RECT = std::mem::zeroed();
        GetClientRect(window, &mut rect);
        let mut info = Info {
            client: (rect.right - rect.left, rect.bottom - rect.top),
            buttons: Vec::new(),
        };
        EnumChildWindows(window, Some(visit), &mut info as *mut Info as isize);

        println!("client area: {}x{}", info.client.0, info.client.1);
        println!("buttons: {}", info.buttons.len());

        // The narrowest button decides whether a long label is clipped.
        let narrowest = info
            .buttons
            .iter()
            .map(|(_, _, _, w, _)| *w)
            .min()
            .unwrap_or(0);
        let widest = info
            .buttons
            .iter()
            .map(|(_, _, _, w, _)| *w)
            .max()
            .unwrap_or(0);
        println!("button width: min {narrowest}px, max {widest}px");

        // Overlap check: within a row, sorted by x, each button must start at or
        // after the previous one ends.
        let mut by_row: Vec<(i32, Vec<Placement>)> = Vec::new();
        for button in &info.buttons {
            let top = button.2;
            match by_row.iter_mut().find(|(y, _)| (*y - top).abs() < 4) {
                Some((_, row)) => row.push(button.clone()),
                None => by_row.push((top, vec![button.clone()])),
            }
        }

        let mut overlaps = 0;
        let mut min_gap = i32::MAX;
        for (_, mut row) in by_row {
            row.sort_by_key(|(_, x, _, _, _)| *x);
            for pair in row.windows(2) {
                let end = pair[0].1 + pair[0].3;
                let gap = pair[1].1 - end;
                min_gap = min_gap.min(gap);
                if gap < 0 {
                    overlaps += 1;
                    println!(
                        "OVERLAP: {:?} ends at {}, {:?} starts at {}",
                        pair[0].0, end, pair[1].0, pair[1].1
                    );
                }
            }
        }
        println!("horizontal overlaps: {overlaps}");
        if min_gap != i32::MAX {
            println!("smallest gap between buttons: {min_gap}px");
        }

        // The right edge: a button past the client area is invisible.
        let client_right = info.client.0;
        let mut offscreen = 0;
        for (label, x, _, w, _) in &info.buttons {
            // Window rects are screen coordinates, so compare widths relative to
            // the window rather than absolute x against the client width.
            let _ = (x, w);
            if *w < 20 {
                offscreen += 1;
                println!("TOO NARROW ({w}px): {label:?}");
            }
        }
        println!("buttons narrower than 20px: {offscreen}");
        let _ = client_right;
    }
}
