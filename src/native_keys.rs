//! Observe Win32 keyboard messages without consuming egui's input.
use egui::Modifiers;

#[derive(Clone, Copy, Debug)]
pub enum NativeKeyEvent {
    Key {
        virtual_key: u16,
        scan_code: u8,
        extended: bool,
        pressed: bool,
        repeat: bool,
        modifiers: Modifiers,
    },
    Char {
        utf16: u16,
        repeat: bool,
        modifiers: Modifiers,
    },
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        rc::Rc,
    };
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::Threading::GetCurrentThreadId,
        UI::{
            Input::KeyboardAndMouse::{
                GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
            },
            Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                GetWindowThreadProcessId, WM_CHAR, WM_KEYDOWN, WM_KEYUP, WM_NCDESTROY, WM_SYSCHAR,
                WM_SYSKEYDOWN, WM_SYSKEYUP,
            },
        },
    };

    const SUBCLASS_ID: usize = 0x5256_534B;
    const MAX_QUEUED: usize = 4096;
    struct State {
        queue: RefCell<VecDeque<NativeKeyEvent>>,
        attached: Cell<bool>,
        thread: u32,
    }

    /// Attach once on the window's UI thread. Rc deliberately makes this !Send.
    pub struct NativeKeys {
        hwnd: HWND,
        state: Rc<State>,
    }
    impl NativeKeys {
        pub fn attach(hwnd: HWND) -> Result<Self, String> {
            unsafe {
                let thread = GetCurrentThreadId();
                if hwnd.is_null() || GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) != thread
                {
                    return Err("键盘记录器必须安装在窗口所属线程".into());
                }
                let mut previous = 0;
                if GetWindowSubclass(hwnd, Some(callback), SUBCLASS_ID, &mut previous) != 0 {
                    return Err("此窗口已经安装键盘记录器".into());
                }
                let state = Rc::new(State {
                    queue: RefCell::new(VecDeque::new()),
                    attached: Cell::new(true),
                    thread,
                });
                // The installation owns one Rc until detach OR WM_NCDESTROY.
                let reference = Rc::into_raw(state.clone());
                if SetWindowSubclass(hwnd, Some(callback), SUBCLASS_ID, reference as usize) == 0 {
                    drop(Rc::from_raw(reference));
                    return Err(format!(
                        "无法安装键盘记录器：{}",
                        std::io::Error::last_os_error()
                    ));
                }
                Ok(Self { hwnd, state })
            }
        }

        pub fn drain(&self) -> Vec<NativeKeyEvent> {
            self.state.queue.borrow_mut().drain(..).collect()
        }
    }

    impl Drop for NativeKeys {
        fn drop(&mut self) {
            unsafe {
                if self.state.thread == GetCurrentThreadId()
                    && self.state.attached.get()
                    && RemoveWindowSubclass(self.hwnd, Some(callback), SUBCLASS_ID) != 0
                {
                    self.state.attached.set(false);
                    drop(Rc::from_raw(Rc::as_ptr(&self.state)));
                }
                // Failed/wrong-thread detach leaves the callback's Rc alive;
                // WM_NCDESTROY will release it on the owning thread.
            }
        }
    }

    unsafe fn modifiers() -> Modifiers {
        let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
        Modifiers {
            ctrl,
            shift: GetKeyState(VK_SHIFT as i32) < 0,
            alt: GetKeyState(VK_MENU as i32) < 0,
            mac_cmd: GetKeyState(VK_LWIN as i32) < 0 || GetKeyState(VK_RWIN as i32) < 0,
            command: ctrl,
        }
    }

    unsafe extern "system" fn callback(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        reference: usize,
    ) -> LRESULT {
        let state = &*(reference as *const State);
        if message == WM_NCDESTROY {
            let release = state.attached.replace(false);
            RemoveWindowSubclass(hwnd, Some(callback), SUBCLASS_ID);
            let result = DefSubclassProc(hwnd, message, wparam, lparam);
            if release {
                drop(Rc::from_raw(reference as *const State));
            }
            return result;
        }
        let flags = lparam as usize;
        let event = match message {
            WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => Some(NativeKeyEvent::Key {
                virtual_key: wparam as u16,
                scan_code: ((flags >> 16) & 0xff) as u8,
                extended: flags & (1 << 24) != 0,
                pressed: message == WM_KEYDOWN || message == WM_SYSKEYDOWN,
                repeat: flags & (1 << 30) != 0,
                modifiers: modifiers(),
            }),
            WM_CHAR | WM_SYSCHAR => Some(NativeKeyEvent::Char {
                utf16: wparam as u16,
                repeat: flags & (1 << 30) != 0,
                modifiers: modifiers(),
            }),
            _ => None,
        };
        if let Some(event) = event {
            let mut queue = state.queue.borrow_mut();
            if queue.len() == MAX_QUEUED {
                queue.pop_front();
            }
            queue.push_back(event);
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

#[cfg(windows)]
pub use platform::NativeKeys;

#[cfg(not(windows))]
pub struct NativeKeys;
#[cfg(not(windows))]
impl NativeKeys {
    pub fn drain(&self) -> Vec<NativeKeyEvent> {
        Vec::new()
    }
}
