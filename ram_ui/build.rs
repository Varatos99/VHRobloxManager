//! Build script to:
//! 1. Copy webview_host.py to output directory
//! 2. Embed Windows icon for exe (using winresource)
//! 3. Embed manifest for admin privileges

#[cfg(windows)]
extern crate winresource;

fn main() {
    // --- Task 1: Copy webview_host.py ---
    copy_webview_script();

    // --- Task 2: Embed Windows icon ---
    embed_icon();

    // --- Task 3: Embed manifest for admin privileges ---
    embed_manifest();
}

fn copy_webview_script() {
    // Get the package directory (where Cargo.toml is)
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let pkg_dir = std::path::Path::new(&manifest_dir);

    // Source file: webview_host.py in package root
    let src = pkg_dir.join("..").join("webview_host.py");

    // Destination: target directory
    let out_dir = std::env::var("OUT_DIR").unwrap_or_default();
    let out_path = std::path::Path::new(&out_dir);

    // Walk up to find target directory
    let mut target_dir = out_path.to_path_buf();
    while target_dir.file_name().is_some() && target_dir.file_name().unwrap() != "target" {
        target_dir = target_dir.parent().unwrap_or(&target_dir).to_path_buf();
    }

    if target_dir.file_name().is_some() {
        let release_dir = target_dir.join("release");
        if release_dir.exists() || std::fs::create_dir_all(&release_dir).is_ok() {
            let dest = release_dir.join("webview_host.py");
            let _ = std::fs::copy(&src, &dest);
        }
    }

    // Also copy to current exe directory for development
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let dest = exe_dir.join("webview_host.py");
            let _ = std::fs::copy(&src, &dest);
        }
    }
}

fn embed_icon() {
    // Only run on Windows
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }

    #[cfg(windows)]
    {
        // Get the package directory to construct path to icon
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        // Icon is in project root's assets folder (../../assets/ from ram_ui/)
        let icon_path = std::path::Path::new(&manifest_dir)
            .join("..")
            .join("assets")
            .join("Logo.ico");

        if !icon_path.exists() {
            panic!("Icon file not found: {}", icon_path.display());
        }

        let mut res = winresource::WindowsResource::new();
        // Use absolute path for the icon
        res.set_icon(icon_path.to_str().unwrap());
        // Let winresource read metadata from Cargo.toml automatically
        res.compile().unwrap();
    }
}

fn embed_manifest() {
    // Manifesti exe yanına kopyala (admin için gerekli)
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let manifest_path = std::path::Path::new(&manifest_dir)
        .join("assets")
        .join("VHRobloxManager.exe.manifest");

    if !manifest_path.exists() {
        return;
    }

    // Find the output exe directory
    let out_dir = std::env::var("OUT_DIR").unwrap_or_default();
    let target_dir = std::path::Path::new(&out_dir);

    let mut release_dir = target_dir.to_path_buf();
    while release_dir.file_name().is_some() && release_dir.file_name().unwrap() != "target" {
        release_dir = release_dir.parent().unwrap_or(&release_dir).to_path_buf();
    }
    release_dir = release_dir.join("release");

    if release_dir.exists() {
        // Copy manifest next to exe
        let dest = release_dir.join("VHRobloxManager.exe.manifest");
        let _ = std::fs::copy(&manifest_path, &dest);

        // Embed manifest into exe using mt.exe (Windows SDK tool)
        let exe_path = release_dir.join("VHRobloxManager.exe");
        if exe_path.exists() {
            let mt_path = std::path::Path::new(&manifest_dir)
                .join("..")
                .join("mt.exe");

            // Try common Windows SDK paths for mt.exe
            let sdk_paths = [
                r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\mt.exe",
                r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.22000.0\x64\mt.exe",
                r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\x64\mt.exe",
                r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.18362.0\x64\mt.exe",
            ];

            for sdk_path in &sdk_paths {
                let sdk_mt = std::path::Path::new(sdk_path);
                if sdk_mt.exists() {
                    // Run mt.exe to embed manifest
                    let status = std::process::Command::new(sdk_mt)
                        .args(&[
                            "-manifest",
                            manifest_path.to_str().unwrap(),
                            "-outputresource:release\\VHRobloxManager.exe;#1",
                        ])
                        .current_dir(&release_dir)
                        .status();

                    if status.is_ok() {
                        break;
                    }
                }
            }
        }
    }
}
