#[cfg(windows)]
fn try_build_windows_resources() {
    use std::path::PathBuf;
    use std::process::Command;

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());

    let manifest_path = PathBuf::from(manifest_dir);
    let out_path = PathBuf::from(out_dir);

    // Compile resources.rc to resources.o
    let rc_file = manifest_path.join("resources.rc");
    let obj_file = out_path.join("resources.o");

    let mut candidates: Vec<String> = Vec::new();
    if let Ok(custom) = std::env::var("WINDRES") {
        if !custom.trim().is_empty() {
            candidates.push(custom);
        }
    }
    candidates.push("windres".to_string());
    candidates.push("x86_64-w64-mingw32-windres".to_string());
    candidates.push(r"C:\msys64\mingw64\bin\windres.exe".to_string());
    candidates.push(r"C:\msys64\usr\bin\windres.exe".to_string());

    let mut last_error: Option<String> = None;
    for candidate in candidates {
        // Skip missing absolute path candidates quickly.
        if (candidate.contains('\\') || candidate.contains('/'))
            && !std::path::Path::new(&candidate).exists()
        {
            continue;
        }

        match Command::new(&candidate)
            .args([
                "-i",
                rc_file.to_string_lossy().as_ref(),
                "-o",
                obj_file.to_string_lossy().as_ref(),
                "-O",
                "coff",
            ])
            .status()
        {
            Ok(status) if status.success() => {
                // Tell cargo to link the object file
                println!("cargo:rustc-link-arg={}", obj_file.display());
                return;
            }
            Ok(status) => {
                last_error = Some(format!(
                    "tool '{}' exited with status {}",
                    candidate, status
                ));
            }
            Err(e) => {
                last_error = Some(format!("tool '{}' failed to start: {}", candidate, e));
            }
        }
    }

    println!("cargo:warning=Windows resource compilation skipped (windres not available).");
    if let Some(err) = last_error {
        println!("cargo:warning=Last windres error: {}", err);
    }
    println!("cargo:warning=Install MinGW/binutils or set WINDRES to enable app icon embedding.");
}

fn main() {
    // Only build resources on Windows.
    #[cfg(windows)]
    {
        try_build_windows_resources();
    }

    println!("cargo:rerun-if-changed=resources.rc");
    println!("cargo:rerun-if-changed=LogInsight_original.png");
}
