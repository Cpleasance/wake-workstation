use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let out_dir = std::env::var("OUT_DIR").unwrap();

        println!("cargo:rustc-link-arg={}/gpu_exports.def", manifest_dir);
        println!("cargo:rerun-if-changed=gpu_exports.def");
        println!("cargo:rerun-if-changed=app.rc");
        println!("cargo:rerun-if-changed=assets/icon.ico");

        let res_path = format!("{}/app.res", out_dir);
        let status = Command::new("windres")
            .args(["app.rc", "-O", "coff", "-o", &res_path])
            .current_dir(&manifest_dir)
            .status();

        if let Ok(s) = status {
            if s.success() {
                println!("cargo:rustc-link-arg={}", res_path);
            }
        }
    }
}
