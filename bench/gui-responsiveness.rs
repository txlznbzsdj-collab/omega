// Proves the window stays responsive while a heavy expression is evaluated.
//
// Types an expensive expression into the running window, then hammers it with
// WM_NULL and measures how long each round trip takes. A window evaluating on
// its own message thread cannot answer until the computation finishes, so a
// multi-second stall shows up immediately as a slow round trip.

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn main() {
    unsafe {
        let class = wide("OmegaWindowClass");
        let window = FindWindowW(class.as_ptr(), std::ptr::null());
        if window.is_null() {
            println!("no omega window is open");
            std::process::exit(1);
        }

        // The input box is the first child edit control.
        let mut input: HWND = std::ptr::null_mut();
        let mut found = 0;
        let mut child = GetWindow(window, GW_CHILD);
        while !child.is_null() {
            let mut buffer = [0u16; 64];
            let n = GetClassNameW(child, buffer.as_mut_ptr(), buffer.len() as i32);
            if String::from_utf16_lossy(&buffer[..n.max(0) as usize]) == "Edit" {
                found += 1;
                if found == 1 {
                    input = child;
                    break;
                }
            }
            child = GetWindow(child, GW_HWNDNEXT);
        }
        if input.is_null() {
            println!("no input box found");
            std::process::exit(1);
        }

        let expression = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "2^4000000".to_string());
        println!("setting the input to {expression:?}");

        let start = std::time::Instant::now();
        SetWindowTextW(input, wide(&expression).as_ptr());

        // Poll the window for a few seconds. Each SendMessage must return
        // promptly if the window is pumping messages.
        let mut worst_ms = 0.0f64;
        let mut probes = 0u32;
        while start.elapsed().as_secs_f64() < 6.0 {
            let t = std::time::Instant::now();
            SendMessageW(window, WM_NULL, 0, 0);
            let elapsed = t.elapsed().as_secs_f64() * 1000.0;
            if elapsed > worst_ms {
                worst_ms = elapsed;
            }
            probes += 1;
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        println!("probes sent: {probes}");
        println!("worst round trip: {worst_ms:.1} ms");

        // A window blocked on a computation cannot answer at all during it; a
        // responsive one answers in well under a frame.
        if worst_ms > 500.0 {
            println!("\nFROZEN: the window stopped responding for {worst_ms:.0} ms");
            std::process::exit(1);
        }
        println!("\nRESPONSIVE: the message loop kept running throughout");
    }
}
