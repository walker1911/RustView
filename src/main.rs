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
mod pages;
mod printing;
mod precision;
mod recipes;
mod shortcuts;
mod worker;

fn main() -> eframe::Result {
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
    if std::env::args().nth(1).as_deref() == Some("--probe") {
        worker::probe_main();
        return Ok(());
    }
    let initial = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let viewer = app::Viewer::start(initial);
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
    #[cfg(windows)]
    if direct_image && eframe::wgpu::Backends::from_env().is_none() {
        // Avoid probing unrelated graphics backends on the direct-image path.
        let mut preferred = options.clone();
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) =
            &mut preferred.wgpu_options.wgpu_setup
        {
            setup.instance_descriptor.backends = eframe::wgpu::Backends::VULKAN;
            let result = run_viewer(preferred, &mut viewer);
            // If renderer initialization failed, the factory has not consumed the Viewer.
            if !matches!(result, Err(eframe::Error::Wgpu(_))) || viewer.is_none() {
                return result;
            }
        }
    }
    run_viewer(options, &mut viewer)
}

fn run_viewer(options: eframe::NativeOptions, viewer: &mut Option<app::Viewer>) -> eframe::Result {
    eframe::run_native(
        "RustView 图片浏览器",
        options,
        Box::new(move |cc| {
            Ok(Box::new(
                viewer
                    .take()
                    .expect("eframe creates one RustView window")
                    .attach_window(cc),
            ))
        }),
    )
}
