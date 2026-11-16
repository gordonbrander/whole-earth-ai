// Links libzim: a prebuilt one from $LIBZIM_DIR (CI, release archives), or the
// system one found with pkg-config (`brew install libzim`, `apt install libzim-dev`).
use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=LIBZIM_DIR");
    println!("cargo:rerun-if-changed=src/zim/shim.cpp");
    println!("cargo:rerun-if-changed=src/zim/shim.h");

    let mut build = cxx_build::bridge("src/zim/mod.rs");
    build.file("src/zim/shim.cpp").std("c++17");

    if let Ok(dir) = env::var("LIBZIM_DIR") {
        let dir = PathBuf::from(dir);
        build.include(dir.join("include"));
        for lib in [
            "lib",
            "lib64",
            "lib/x86_64-linux-gnu",
            "lib/aarch64-linux-gnu",
        ] {
            println!("cargo:rustc-link-search=native={}", dir.join(lib).display());
        }
    } else {
        // Only libzim's own flags: its private deps (ICU, Xapian) are linked into libzim.
        let lib = pkg_config::Config::new()
            .env_metadata(true)
            .print_system_libs(false)
            .cargo_metadata(false)
            .probe("libzim")
            .expect("libzim not found: install it (brew install libzim / apt install libzim-dev) or set LIBZIM_DIR");
        for p in lib.include_paths {
            build.include(p);
        }
        for p in lib.link_paths {
            println!("cargo:rustc-link-search=native={}", p.display());
        }
    }
    build.compile("zim-shim");
    // After the shim: GNU ld resolves left to right, so libzim must follow it.
    println!("cargo:rustc-link-lib=zim");

    // Find libzim next to the binary in release archives.
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path"),
        Ok("linux") => println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN"),
        _ => {}
    }
}
