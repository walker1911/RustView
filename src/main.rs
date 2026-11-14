#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod animation;
mod animation_player;
mod app;
mod archive;
mod catalog;
mod clipboard;
mod desktop;
mod editing;
mod fileops;
mod imaging;
mod native_keys;
mod metadata;
mod legacy_settings;
mod ocr;
mod pages;
mod pyramid;
mod pyramid_worker;
mod printing;
mod psd_composite;
mod psd_view;
mod precision;
mod qr;
mod recipes;
mod shortcuts;
mod svg;
mod worker;

static STARTUP_TRACE: std::sync::OnceLock<(std::time::Instant, std::path::PathBuf)> =
    std::sync::OnceLock::new();

fn trace_startup(stage: &str) {
    use std::io::Write;
    if let Some((started, path)) = STARTUP_TRACE.get() {
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let wall = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros();
            let line = format!("{{\"stage\":\"{stage}\",\"elapsed_us\":{},\"unix_us\":{wall}}}\n", started.elapsed().as_micros());
            let _ = file.write_all(line.as_bytes());
        }
    }
}

fn main() -> eframe::Result {
    let started = std::time::Instant::now();
    if std::env::args().nth(1).as_deref() == Some("--navigation-probe") {
        app::navigation_probe_main();
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--request-probe") {
        worker::request_probe_main();
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--animation-probe") {
        animation_player::probe_main();
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--animation-worker") {
        animation_player::worker_main();
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--worker") {
        worker::worker_main();
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--pyramid-tiles") {
        pyramid_worker::worker_main();
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--probe") {
        worker::probe_main();
        return Ok(());
    }
    if let Some(path) = std::env::var_os("RUSTVIEW_STARTUP_TRACE") {
        let _ = STARTUP_TRACE.set((started, path.into()));
    }
    trace_startup("main");
    let initial = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let viewer = app::Viewer::start(initial);
    trace_startup("viewer_initialized");
    #[cfg(windows)]
    let direct_image = viewer.is_direct_image_startup();
    let mut viewer = Some(viewer);
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/rustview-icon.png"))
        .expect("embedded RustView icon must be a valid PNG");
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 840.0])
            .with_min_inner_size([720.0, 480.0])
            .with_drag_and_drop(true)
            .with_icon(icon),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    options.wgpu_options.desired_maximum_frame_latency = Some(1);
    trace_startup("window_setup");
    #[cfg(windows)]
    if direct_image && eframe::wgpu::Backends::from_env().is_none() {
        use eframe::wgpu::{Backends, DeviceType};

        for backend in [Backends::GL, Backends::VULKAN] {
            let mut preferred = options.clone();
            let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) =
                &mut preferred.wgpu_options.wgpu_setup
            else {
                break;
            };
            setup.instance_descriptor.backends = backend;
            if backend == Backends::GL {
                // Use the adapter already enumerated by egui-wgpu. Software GL can
                // open a window but makes later zooming and tile uploads expensive.
                setup.native_adapter_selector = Some(std::sync::Arc::new(|adapters, surface| {
                    trace_startup("gl_enumerated");
                    let selected = adapters
                        .iter()
                        .find(|adapter| {
                            let info = adapter.get_info();
                            info.device_type != DeviceType::Cpu
                                && info.vendor != 0
                                && surface.is_none_or(|surface| adapter.is_surface_supported(surface))
                                && opengl_supports_vsync(adapter)
                        })
                        .cloned()
                        .ok_or_else(|| "No compatible hardware OpenGL adapter".into());
                    trace_startup("gl_selected");
                    selected
                }));
            }
            trace_startup(if backend == Backends::GL { "gl_begin" } else { "vulkan_begin" });
            let result = run_viewer(preferred, &mut viewer);
            // If renderer initialization failed, the factory has not consumed the Viewer.
            if !matches!(result, Err(eframe::Error::Wgpu(_))) || viewer.is_none() {
                return result;
            }
        }
    }
    run_viewer(options, &mut viewer)
}

#[cfg(windows)]
fn opengl_supports_vsync(adapter: &eframe::wgpu::Adapter) -> bool {
    use std::ffi::{c_char, CStr};
    use windows::Win32::Graphics::{Gdi::HDC, OpenGL::*};

    // egui-wgpu configures the surface before calling our app factory. Check
    // WGL swap control while we can still reject the adapter with a regular error.
    unsafe {
        adapter.as_hal::<eframe::wgpu::hal::api::Gles, _, _>(|adapter| {
            let Some(adapter) = adapter else { return false };
            let _context = adapter.adapter_context().lock();
            let Some(extensions) = wglGetProcAddress(windows::core::s!("wglGetExtensionsStringARB"))
            else {
                return false;
            };
            let Some(swap_interval) = wglGetProcAddress(windows::core::s!("wglSwapIntervalEXT"))
            else {
                return false;
            };
            if [extensions as usize, swap_interval as usize]
                .into_iter()
                .any(|address| address <= 3 || address == usize::MAX)
            {
                return false;
            }
            let extensions: unsafe extern "system" fn(HDC) -> *const c_char =
                std::mem::transmute(extensions);
            let extensions = extensions(wglGetCurrentDC());
            !extensions.is_null()
                && CStr::from_ptr(extensions)
                    .to_bytes()
                    .split(|byte| *byte == b' ')
                    .any(|name| name == b"WGL_EXT_swap_control")
        })
    }
}

fn run_viewer(options: eframe::NativeOptions, viewer: &mut Option<app::Viewer>) -> eframe::Result {
    eframe::run_native(
        "RustView 图片浏览器",
        options,
        Box::new(move |cc| {
            trace_startup("renderer_ready");
            Ok(Box::new(
                viewer
                    .take()
                    .expect("eframe creates one RustView window")
                    .attach_window(cc),
            ))
        }),
    )
}
