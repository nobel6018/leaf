//! macOS에서 ONNX Runtime CoreML 실행기가 쓰는 `__isPlatformVersionAtLeast`(Objective-C `@available`)는
//! clang 런타임(libclang_rt.osx.a)에 있다. 개발 빌드는 우연히 링크되지만 LTO를 켠 release 빌드에서는
//! 심볼을 못 찾아 링크가 깨져서 직접 붙인다.
//!
//! @see docs/vision.md

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let Ok(out) = Command::new("clang").arg("--print-resource-dir").output() else {
        println!("cargo:warning=clang을 찾지 못해 libclang_rt.osx를 붙이지 않았습니다");
        return;
    };
    let dir = std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()).join("lib/darwin");
    if dir.join("libclang_rt.osx.a").is_file() {
        println!("cargo:rustc-link-search=native={}", dir.display());
        println!("cargo:rustc-link-lib=static=clang_rt.osx");
    } else {
        println!("cargo:warning=libclang_rt.osx.a가 없습니다: {}", dir.display());
    }
}
