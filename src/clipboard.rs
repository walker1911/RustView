use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct FileClipboard {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
    pub sequence: u32,
}

pub struct ClipboardImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[cfg(windows)]
fn file_list(paths: &[PathBuf]) -> Result<Vec<u16>, String> {
    use std::os::windows::ffi::OsStrExt;

    if paths.is_empty() {
        return Err("没有可复制的文件".into());
    }
    if paths.len() > 100_000 {
        return Err("剪贴板文件数量超过十万条上限".into());
    }
    let mut list = Vec::new();
    for path in paths {
        if path.as_os_str().is_empty() {
            return Err("复制文件的路径不能为空".into());
        }
        let path = std::path::absolute(path).map_err(|e| e.to_string())?;
        let encoded: Vec<u16> = path.as_os_str().encode_wide().collect();
        if encoded.contains(&0) {
            return Err("复制文件的路径包含空字符".into());
        }
        if list.len().saturating_add(encoded.len()).saturating_add(2) > 8 * 1024 * 1024 {
            return Err("剪贴板文件路径总长度超过16MiB上限".into());
        }
        list.extend(encoded);
        list.push(0);
    }
    list.push(0); // CF_HDROP ends the list with an additional UTF-16 NUL.
    Ok(list)
}

/// Copy file references as Unicode CF_HDROP, not as image data or path text.
/// Call on the viewer UI thread, whose active window becomes clipboard owner.
/// Files must remain available while other applications may paste them.
#[cfg(windows)]
pub fn copy_files(paths: &[PathBuf]) -> Result<(), String> {
    set_files(paths, false, None).map(|_| ())
}

#[cfg(windows)]
pub fn cut_files(paths: &[PathBuf]) -> Result<(), String> {
    set_files(paths, true, None).map(|_| ())
}

#[cfg(windows)]
pub fn finish_cut(original: &FileClipboard, destinations: &[PathBuf]) -> Result<bool, String> {
    if !original.cut || original.paths.len() != destinations.len() {
        return Ok(false);
    }
    set_files(destinations, false, Some(original.sequence))
}

#[cfg(windows)]
fn set_files(paths: &[PathBuf], cut: bool, expected_sequence: Option<u32>) -> Result<bool, String> {
    use std::{mem::size_of, ptr};
    use windows_sys::Win32::{
        Foundation::{GetLastError, GlobalFree, SetLastError, HGLOBAL, POINT},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
                RegisterClipboardFormatW, SetClipboardData,
            },
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        },
        UI::{Input::KeyboardAndMouse::GetActiveWindow, Shell::DROPFILES},
    };

    // Standard clipboard format from WinUser.h; avoids the otherwise unused Ole feature.
    const CF_HDROP: u32 = 15;
    fn os_error(operation: &str) -> String {
        format!("{operation}：{}", std::io::Error::last_os_error())
    }
    struct Memory {
        handle: HGLOBAL,
        locked: bool,
    }
    impl Drop for Memory {
        fn drop(&mut self) {
            unsafe {
                if self.locked {
                    GlobalUnlock(self.handle);
                }
                if !self.handle.is_null() {
                    GlobalFree(self.handle);
                }
            }
        }
    }
    struct Clipboard;
    impl Drop for Clipboard {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }

    let list = file_list(paths)?;
    let size = list
        .len()
        .checked_mul(size_of::<u16>())
        .and_then(|bytes| bytes.checked_add(size_of::<DROPFILES>()))
        .ok_or("复制文件列表过大")?;
    unsafe {
        // OpenClipboard(NULL) is not valid for this write sequence: EmptyClipboard
        // would leave no owner, causing SetClipboardData to fail (Win32 contract).
        let owner = GetActiveWindow();
        if owner.is_null() {
            return Err("没有可用于复制文件的活动窗口，请返回图片窗口重试".into());
        }
        let mut memory = Memory {
            handle: GlobalAlloc(GMEM_MOVEABLE, size),
            locked: false,
        };
        if memory.handle.is_null() {
            return Err(os_error("无法分配剪贴板内存"));
        }
        let data = GlobalLock(memory.handle).cast::<u8>();
        if data.is_null() {
            return Err(os_error("无法锁定剪贴板内存"));
        }
        memory.locked = true;
        ptr::write(
            data.cast::<DROPFILES>(),
            DROPFILES {
                pFiles: size_of::<DROPFILES>() as u32,
                pt: POINT { x: 0, y: 0 },
                fNC: 0,
                fWide: 1,
            },
        );
        ptr::copy_nonoverlapping(
            list.as_ptr(),
            data.add(size_of::<DROPFILES>()).cast::<u16>(),
            list.len(),
        );
        SetLastError(0);
        let remaining_locks = GlobalUnlock(memory.handle);
        // Zero also means successful final unlock; only nonzero last-error fails.
        if remaining_locks == 0 && GetLastError() != 0 {
            return Err(os_error("无法解锁剪贴板内存"));
        }
        memory.locked = false;

        if OpenClipboard(owner) == 0 {
            return Err(os_error("无法打开系统剪贴板"));
        }
        let _clipboard = Clipboard;
        if expected_sequence.is_some_and(|sequence| GetClipboardSequenceNumber() != sequence) {
            return Ok(false);
        }
        if EmptyClipboard() == 0 {
            return Err(os_error("无法清空系统剪贴板"));
        }
        if SetClipboardData(CF_HDROP, memory.handle).is_null() {
            return Err(os_error("无法复制文件到系统剪贴板"));
        }
        // Successful SetClipboardData transfers ownership. Never free this handle.
        memory.handle = ptr::null_mut();

        let format_name: Vec<u16> = "Preferred DropEffect\0".encode_utf16().collect();
        let format = RegisterClipboardFormatW(format_name.as_ptr());
        if format == 0 {
            return Err(os_error("无法注册剪切状态"));
        }
        let mut effect = Memory {
            handle: GlobalAlloc(GMEM_MOVEABLE, 4),
            locked: false,
        };
        if effect.handle.is_null() {
            return Err(os_error("无法分配剪切状态"));
        }
        let data = GlobalLock(effect.handle).cast::<u32>();
        if data.is_null() {
            return Err(os_error("无法锁定剪切状态"));
        }
        effect.locked = true;
        ptr::write(data, if cut { 2 } else { 1 });
        GlobalUnlock(effect.handle);
        effect.locked = false;
        if SetClipboardData(format, effect.handle).is_null() {
            return Err(os_error("无法设置剪切状态"));
        }
        effect.handle = ptr::null_mut();
    }
    Ok(true)
}

#[cfg(not(windows))]
pub fn copy_files(_paths: &[PathBuf]) -> Result<(), String> {
    Err("文件剪贴板仅支持 Windows".into())
}

#[cfg(not(windows))]
pub fn cut_files(_paths: &[PathBuf]) -> Result<(), String> {
    Err("文件剪贴板仅支持 Windows".into())
}

#[cfg(not(windows))]
pub fn finish_cut(_original: &FileClipboard, _destinations: &[PathBuf]) -> Result<bool, String> {
    Ok(false)
}

#[cfg(windows)]
struct OpenedClipboard;

#[cfg(windows)]
impl OpenedClipboard {
    fn open() -> Result<Self, String> {
        if unsafe { windows_sys::Win32::System::DataExchange::OpenClipboard(std::ptr::null_mut()) }
            == 0
        {
            return Err(format!(
                "无法读取剪贴板：{}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self)
    }
}

#[cfg(windows)]
impl Drop for OpenedClipboard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::DataExchange::CloseClipboard();
        }
    }
}

#[cfg(windows)]
pub fn read_files() -> Result<Option<FileClipboard>, String> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::{
        System::{
            DataExchange::{
                GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
                RegisterClipboardFormatW,
            },
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        },
        UI::Shell::DragQueryFileW,
    };
    let _clipboard = OpenedClipboard::open()?;
    unsafe {
        if IsClipboardFormatAvailable(15) == 0 {
            return Ok(None);
        }
        let handle = GetClipboardData(15);
        if handle.is_null() {
            return Err("剪贴板文件列表不可用".into());
        }
        if GlobalSize(handle) > 16 * 1024 * 1024 {
            return Err("剪贴板文件列表超过16MiB上限".into());
        }
        let count = DragQueryFileW(handle, u32::MAX, std::ptr::null_mut(), 0);
        if count > 100_000 {
            return Err("剪贴板文件数量超过十万条上限".into());
        }
        let mut paths = Vec::new();
        let mut total = 0_usize;
        for index in 0..count {
            let length = DragQueryFileW(handle, index, std::ptr::null_mut(), 0);
            if length == 0 || length > 32768 {
                return Err("剪贴板文件路径无效或过长".into());
            }
            total = total.saturating_add(length as usize + 1);
            if total > 8 * 1024 * 1024 {
                return Err("剪贴板路径总长度超过16MiB上限".into());
            }
            let mut buffer = vec![0; length as usize + 1];
            if DragQueryFileW(handle, index, buffer.as_mut_ptr(), length + 1) != length {
                return Err("剪贴板文件列表不完整".into());
            }
            paths.push(PathBuf::from(OsString::from_wide(
                &buffer[..length as usize],
            )));
        }
        let format_name: Vec<u16> = "Preferred DropEffect\0".encode_utf16().collect();
        let effect = GetClipboardData(RegisterClipboardFormatW(format_name.as_ptr()));
        let cut = if !effect.is_null() && GlobalSize(effect) >= 4 {
            let data = GlobalLock(effect).cast::<u32>();
            if data.is_null() {
                false
            } else {
                let cut = std::ptr::read_unaligned(data) & 2 != 0;
                GlobalUnlock(effect);
                cut
            }
        } else {
            false
        };
        Ok(Some(FileClipboard {
            paths,
            cut,
            sequence: GetClipboardSequenceNumber(),
        }))
    }
}

#[cfg(not(windows))]
pub fn read_files() -> Result<Option<FileClipboard>, String> {
    Ok(None)
}

fn dib_dimensions(header: &[u8]) -> Result<(u32, u32), String> {
    if header.len() < 12 {
        return Err("剪贴板位图头不完整".into());
    }
    let size = u32::from_le_bytes(header[..4].try_into().unwrap());
    let (width, height) = if size == 12 {
        (
            u16::from_le_bytes(header[4..6].try_into().unwrap()) as u32,
            u16::from_le_bytes(header[6..8].try_into().unwrap()) as u32,
        )
    } else if size >= 40 && header.len() >= 40 {
        let width = i32::from_le_bytes(header[4..8].try_into().unwrap());
        if width <= 0 {
            return Err("剪贴板位图宽度无效".into());
        }
        (
            width as u32,
            i32::from_le_bytes(header[8..12].try_into().unwrap()).unsigned_abs(),
        )
    } else {
        return Err("不支持该剪贴板位图头".into());
    };
    if width == 0 || height == 0 {
        return Err("剪贴板位图尺寸无效".into());
    }
    Ok((width, height))
}

#[cfg(windows)]
fn clipboard_dimensions(max_bytes: usize) -> Result<Option<(u32, u32)>, String> {
    use windows_sys::Win32::{
        Graphics::Gdi::{GetObjectW, BITMAP},
        System::{
            DataExchange::{GetClipboardData, IsClipboardFormatAvailable},
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        },
    };
    let _clipboard = OpenedClipboard::open()?;
    unsafe {
        for format in [17, 8] {
            if IsClipboardFormatAvailable(format) == 0 {
                continue;
            }
            let handle = GetClipboardData(format);
            if handle.is_null() {
                return Err("剪贴板位图不可用".into());
            }
            let length = GlobalSize(handle);
            if length < 12 {
                return Err("剪贴板位图头不完整".into());
            }
            if length > max_bytes.saturating_add(16 * 1024 * 1024) {
                return Err("剪贴板位图数据超过读取预算".into());
            }
            let data = GlobalLock(handle).cast::<u8>();
            if data.is_null() {
                return Err("无法锁定剪贴板位图".into());
            }
            let result = dib_dimensions(std::slice::from_raw_parts(data, length.min(124)));
            GlobalUnlock(handle);
            return result.map(Some);
        }
        if IsClipboardFormatAvailable(2) != 0 {
            let bitmap = GetClipboardData(2);
            let mut dimensions: BITMAP = std::mem::zeroed();
            if bitmap.is_null()
                || GetObjectW(
                    bitmap,
                    std::mem::size_of::<BITMAP>() as i32,
                    (&mut dimensions as *mut BITMAP).cast(),
                ) == 0
                || dimensions.bmWidth <= 0
                || dimensions.bmHeight == 0
            {
                return Err("无法取得剪贴板位图尺寸".into());
            }
            return Ok(Some((
                dimensions.bmWidth as u32,
                dimensions.bmHeight.unsigned_abs(),
            )));
        }
        Ok(None)
    }
}

/// Reads through arboard after a native dimension preflight. Use an isolated worker for a hard memory ceiling.
pub fn read_image(max_bytes: usize, max_pixels: u64) -> Result<Option<ClipboardImage>, String> {
    #[cfg(windows)]
    let dimensions = clipboard_dimensions(max_bytes)?;
    #[cfg(not(windows))]
    let dimensions: Option<(u32, u32)> = None;
    let Some((width, height)) = dimensions else {
        return Ok(None);
    };
    let pixels = (width as u64)
        .checked_mul(height as u64)
        .ok_or("剪贴板图片尺寸溢出")?;
    if pixels > max_pixels || pixels.saturating_mul(4) > max_bytes as u64 {
        return Err("剪贴板图片超过内存或像素预算".into());
    }
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    let image = clipboard.get_image().map_err(|error| error.to_string())?;
    let width = u32::try_from(image.width).map_err(|_| "剪贴板图片宽度过大")?;
    let height = u32::try_from(image.height).map_err(|_| "剪贴板图片高度过大")?;
    let pixels = (width as u64)
        .checked_mul(height as u64)
        .ok_or("剪贴板图片尺寸溢出")?;
    if pixels > max_pixels
        || pixels.saturating_mul(4) > max_bytes as u64
        || image.bytes.len() as u64 != pixels.saturating_mul(4)
    {
        return Err("剪贴板图片超过预算或数据不完整".into());
    }
    Ok(Some(ClipboardImage {
        width,
        height,
        rgba: image.bytes.into_owned(),
    }))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStringExt;

    #[test]
    fn unicode_file_list_is_double_terminated() {
        let paths = [
            PathBuf::from(r"C:\图片\一.png"),
            PathBuf::from(r"C:\图片\二.jpg"),
        ];
        let data = file_list(&paths).unwrap();
        assert_eq!(&data[data.len() - 2..], &[0, 0]);
        let names: Vec<_> = data
            .split(|unit| *unit == 0)
            .filter(|part| !part.is_empty())
            .map(std::ffi::OsString::from_wide)
            .collect();
        assert_eq!(
            names,
            paths
                .iter()
                .map(|p| p.as_os_str().to_owned())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rejects_empty_and_embedded_nul_paths() {
        assert!(file_list(&[]).is_err());
        assert!(file_list(&[PathBuf::new()]).is_err());
        let path = PathBuf::from(std::ffi::OsString::from_wide(&[
            b'C' as u16,
            0,
            b'x' as u16,
        ]));
        assert!(file_list(&[path]).is_err());
    }

    #[test]
    fn dib_preflight_handles_top_down_and_rejects_bad_headers() {
        let mut header = [0_u8; 40];
        header[..4].copy_from_slice(&40_u32.to_le_bytes());
        header[4..8].copy_from_slice(&100_i32.to_le_bytes());
        header[8..12].copy_from_slice(&(-20_i32).to_le_bytes());
        assert_eq!(dib_dimensions(&header).unwrap(), (100, 20));
        header[4..8].copy_from_slice(&(-1_i32).to_le_bytes());
        assert!(dib_dimensions(&header).is_err());
        assert!(dib_dimensions(&[0; 8]).is_err());
    }
}
