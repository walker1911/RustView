fn main() {
    println!("cargo:rerun-if-changed=RustView.manifest");
    println!("cargo:rerun-if-changed=RustView.rc");
    println!("cargo:rerun-if-changed=assets/rustview-icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let project_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let manifest = std::path::Path::new(&project_dir).join("RustView.manifest");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());

        let target = std::env::var("TARGET").unwrap();
        let resource =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("RustView.res");
        let status = find_msvc_tools::find(&target, "rc.exe")
            .expect("Windows SDK resource compiler (rc.exe) was not found")
            .current_dir(project_dir)
            .args(["/nologo", "/fo"])
            .arg(&resource)
            .arg("RustView.rc")
            .status()
            .expect("failed to run the Windows SDK resource compiler");
        assert!(status.success(), "Windows icon resource compilation failed");
        println!("cargo:rustc-link-arg-bin=RustView={}", resource.display());
    }
}
