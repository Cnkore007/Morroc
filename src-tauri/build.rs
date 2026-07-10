use std::env;

fn main() {
    tauri_build::build();

    // 在 Windows 上构建 cdylib 时导出符号过多，导致
    // "export ordinal too large"。显式排除所有符号导出，避免超过 PE
    // 导出表限制。桌面端 Tauri 实际使用 bin + rlib，不需要该 DLL 导出。
    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("windows") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,--exclude-all-symbols");
    }
}
