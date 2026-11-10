use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

const MAX_BACKUP: u64 = 2 * 1024 * 1024;
const MAX_MONITORS: usize = 256;

#[derive(Clone, Copy, Debug)]
pub enum WallpaperMode {
    Center,
    Tile,
    Stretch,
}

#[derive(Serialize, Deserialize)]
struct Monitor {
    id: String,
    path: String,
}

#[derive(Serialize, Deserialize)]
struct Backup {
    version: u32,
    position: i32,
    monitors: Vec<Monitor>,
}

impl Backup {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || !(0..=5).contains(&self.position)
            || self.monitors.is_empty()
            || self.monitors.len() > MAX_MONITORS
        {
            return Err("壁纸备份格式无效".into());
        }
        let mut ids = std::collections::HashSet::new();
        for monitor in &self.monitors {
            if monitor.id.is_empty()
                || !ids.insert(&monitor.id)
                || [&monitor.id, &monitor.path]
                    .iter()
                    .any(|value| value.contains('\0') || value.encode_utf16().count() >= 32768)
            {
                return Err("壁纸备份含无效显示器或路径".into());
            }
        }
        Ok(())
    }
}

fn read_backup(path: &Path) -> Result<Backup, String> {
    let file = File::open(path).map_err(|error| format!("无法读取壁纸备份：{error}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_BACKUP + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BACKUP {
        return Err("壁纸备份超过2MiB上限".into());
    }
    let backup: Backup =
        serde_json::from_slice(&bytes).map_err(|error| format!("壁纸备份格式错误：{error}"))?;
    backup.validate()?;
    Ok(backup)
}

fn save_first_backup(path: &Path, backup: &Backup) -> Result<(), String> {
    if path.exists() {
        read_backup(path)?;
        return Ok(());
    }
    backup.validate()?;
    let bytes = serde_json::to_vec_pretty(backup).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BACKUP {
        return Err("壁纸备份超过2MiB上限".into());
    }
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    temp.write_all(&bytes).map_err(|error| error.to_string())?;
    temp.as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    match temp.persist_noclobber(path) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            read_backup(path).map(|_| ())
        }
        Err(error) => Err(format!("无法保存壁纸备份：{}", error.error)),
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use windows::{
        core::{PCWSTR, PWSTR},
        Win32::{
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
                COINIT_APARTMENTTHREADED,
            },
            UI::Shell::{
                DesktopWallpaper, IDesktopWallpaper, DESKTOP_WALLPAPER_POSITION, DSS_SLIDESHOW,
            },
        },
    };

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    struct ComString(PWSTR);
    impl Drop for ComString {
        fn drop(&mut self) {
            unsafe { CoTaskMemFree(Some(self.0 .0.cast())) };
        }
    }

    fn text(value: PWSTR) -> Result<String, String> {
        let value = ComString(value);
        if value.0.is_null() {
            return Ok(String::new());
        }
        // Windows supplies a CoTaskMem-allocated, NUL-terminated string.
        unsafe { value.0.to_string() }.map_err(|_| "Windows壁纸路径不是有效Unicode".into())
    }

    fn wide(value: &str) -> Result<Vec<u16>, String> {
        let mut value: Vec<u16> = value.encode_utf16().collect();
        if value.contains(&0) || value.len() >= 32768 {
            return Err("壁纸路径过长或含NUL".into());
        }
        value.push(0);
        Ok(value)
    }

    fn run(
        action: impl FnOnce(&IDesktopWallpaper) -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        std::thread::Builder::new()
            .name("wallpaper-com".into())
            .spawn(move || {
                unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }
                    .map_err(|error| format!("无法初始化桌面接口：{error}"))?;
                let _apartment = Apartment;
                let desktop: IDesktopWallpaper =
                    unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }
                        .map_err(|error| format!("无法访问Windows桌面壁纸：{error}"))?;
                action(&desktop)
            })
            .map_err(|error| error.to_string())?
            .join()
            .map_err(|_| "桌面壁纸线程意外结束".to_owned())?
    }

    fn monitor_ids(desktop: &IDesktopWallpaper) -> Result<Vec<String>, String> {
        let count =
            unsafe { desktop.GetMonitorDevicePathCount() }.map_err(|error| error.to_string())?;
        if count == 0 || count as usize > MAX_MONITORS {
            return Err("显示器数量超出支持范围".into());
        }
        (0..count)
            .map(|index| {
                text(
                    unsafe { desktop.GetMonitorDevicePathAt(index) }
                        .map_err(|error| error.to_string())?,
                )
            })
            .collect()
    }

    fn snapshot(desktop: &IDesktopWallpaper) -> Result<Backup, String> {
        let status = unsafe { desktop.GetStatus() }.map_err(|error| error.to_string())?;
        if status.0 & DSS_SLIDESHOW.0 != 0 {
            return Err(
                "当前Windows使用壁纸幻灯片；此版本仅能备份和还原静态壁纸，请先在系统设置中改为图片"
                    .into(),
            );
        }
        let monitors = monitor_ids(desktop)?
            .into_iter()
            .map(|id| {
                let encoded = wide(&id)?;
                let path = text(
                    unsafe { desktop.GetWallpaper(PCWSTR(encoded.as_ptr())) }
                        .map_err(|error| error.to_string())?,
                )?;
                Ok(Monitor { id, path })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let backup = Backup {
            version: 1,
            position: unsafe { desktop.GetPosition() }
                .map_err(|error| error.to_string())?
                .0,
            monitors,
        };
        backup.validate()?;
        Ok(backup)
    }

    fn apply(desktop: &IDesktopWallpaper, backup: &Backup) -> Result<(), String> {
        let ids = monitor_ids(desktop)?;
        let targets: Vec<_> = backup
            .monitors
            .iter()
            .filter(|monitor| ids.contains(&monitor.id))
            .collect();
        if targets.is_empty() {
            return Err("备份中的显示器当前均未连接，尚未修改桌面".into());
        }
        // Complete validation before the first desktop write.
        for target in &targets {
            if !target.path.is_empty() && !Path::new(&target.path).is_file() {
                return Err(format!("原壁纸已不存在，尚未修改桌面：{}", target.path));
            }
        }
        for target in targets {
            let id = wide(&target.id)?;
            let path = wide(&target.path)?;
            unsafe { desktop.SetWallpaper(PCWSTR(id.as_ptr()), PCWSTR(path.as_ptr())) }
                .map_err(|error| format!("无法恢复显示器壁纸：{error}"))?;
        }
        unsafe { desktop.SetPosition(DESKTOP_WALLPAPER_POSITION(backup.position)) }
            .map_err(|error| format!("无法恢复壁纸位置：{error}"))
    }

    pub(super) fn set(path: &Path, mode: WallpaperMode, backup_path: &Path) -> Result<(), String> {
        if !path.is_file() {
            return Err("壁纸文件不存在".into());
        }
        let path = path.canonicalize().map_err(|error| error.to_string())?;
        let path = path.to_str().ok_or("壁纸路径不是有效Unicode")?.to_owned();
        let path = wide(&path)?;
        let backup_path = backup_path.to_owned();
        run(move |desktop| {
            let before = snapshot(desktop)?;
            save_first_backup(&backup_path, &before)?;
            let position = match mode {
                WallpaperMode::Center => 0,
                WallpaperMode::Tile => 1,
                WallpaperMode::Stretch => 2,
            };
            let result = (|| {
                unsafe { desktop.SetWallpaper(PCWSTR::null(), PCWSTR(path.as_ptr())) }
                    .map_err(|error| format!("无法设置壁纸：{error}"))?;
                unsafe { desktop.SetPosition(DESKTOP_WALLPAPER_POSITION(position)) }
                    .map_err(|error| format!("无法设置壁纸位置：{error}"))
            })();
            if let Err(error) = result {
                return Err(match apply(desktop, &before) {
                    Ok(()) => format!("{error}；已还原操作前壁纸"),
                    Err(rollback) => format!("{error}；自动还原失败：{rollback}；原始备份仍保留"),
                });
            }
            Ok(())
        })
    }

    pub(super) fn restore(path: &Path) -> Result<(), String> {
        let backup = read_backup(path)?;
        run(move |desktop| {
            let before = snapshot(desktop)?;
            if let Err(error) = apply(desktop, &backup) {
                return Err(match apply(desktop, &before) {
                    Ok(()) => format!("{error}；已还原操作前壁纸"),
                    Err(rollback) => format!("{error}；自动还原失败：{rollback}；备份仍保留"),
                });
            }
            Ok(())
        })
    }
}

/// The first backup is retained across later changes and restores.
/// Wallpaper files must remain at their saved paths; Windows slideshows are rejected.
pub fn set(path: &Path, mode: WallpaperMode, backup: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        native::set(path, mode, backup)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, mode, backup);
        Err("桌面壁纸仅支持Windows".into())
    }
}

pub fn restore(backup: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        native::restore(backup)
    }
    #[cfg(not(windows))]
    {
        let _ = backup;
        Err("桌面壁纸仅支持Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Backup {
        Backup {
            version: 1,
            position: 4,
            monitors: vec![Monitor {
                id: "显示器一".into(),
                path: "C:\\图片\\原壁纸.bmp".into(),
            }],
        }
    }

    #[test]
    fn first_backup_survives_subsequent_sets_and_restore_reads() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("wallpaper.json");
        save_first_backup(&path, &sample()).unwrap();
        let original = std::fs::read(&path).unwrap();
        let mut later = sample();
        later.position = 1;
        later.monitors[0].path = "C:\\new.bmp".into();
        save_first_backup(&path, &later).unwrap();
        assert_eq!(read_backup(&path).unwrap().position, 4);
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn rejects_corrupt_or_oversized_backup_before_com_calls() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("wallpaper.json");
        std::fs::write(&path, b"{}").unwrap();
        assert!(read_backup(&path).is_err());
        let file = File::create(&path).unwrap();
        file.set_len(MAX_BACKUP + 1).unwrap();
        assert!(read_backup(&path).is_err());
        let mut invalid = sample();
        invalid.monitors.push(Monitor {
            id: "显示器一".into(),
            path: String::new(),
        });
        assert!(invalid.validate().is_err());
        invalid = sample();
        invalid.position = 9;
        assert!(invalid.validate().is_err());
    }
}
