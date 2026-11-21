use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=luma.rc");
    println!("cargo:rerun-if-changed=luma.manifest");
    println!("cargo:rerun-if-changed=luma.ico");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let res = out_dir.join("luma.res");
    let rc = Command::new("rc.exe")
        .arg("/nologo")
        .arg("/fo")
        .arg(&res)
        .arg("luma.rc")
        .status();

    match rc {
        Ok(status) if status.success() => {
            // Use /MANIFEST:NO so the linker keeps the manifest compiled into
            // luma.res instead of generating its own default one.
            println!("cargo:rustc-link-arg=/MANIFEST:NO");
            println!("cargo:rustc-link-arg={}", res.display());
        }
        _ => {
            println!("cargo:warning=rc.exe not found; building without icon/manifest resources");
        }
    }
}
