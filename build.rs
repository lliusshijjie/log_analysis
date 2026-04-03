fn main() {
    // Only build on Windows
    #[cfg(windows)]
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let out_dir = std::env::var("OUT_DIR").unwrap();

        let manifest_path = std::path::Path::new(&manifest_dir);
        let out_path = std::path::Path::new(&out_dir);

        // Compile resources.rc to resources.o
        let rc_file = manifest_path.join("resources.rc");
        let obj_file = out_path.join("resources.o");

        std::process::Command::new("C:\\msys64\\mingw64\\bin\\windres.exe")
            .args(&[
                "-i",
                rc_file.to_str().unwrap(),
                "-o",
                obj_file.to_str().unwrap(),
                "-O",
                "coff",
            ])
            .status()
            .expect("Failed to run windres - make sure mingw or binutils is installed");

        // Tell cargo to link the object file
        println!("cargo:rustc-link-arg={}", obj_file.display());
    }

    println!("cargo:rerun-if-changed=resources.rc");
    println!("cargo:rerun-if-changed=LogInsight_original.png");
}
