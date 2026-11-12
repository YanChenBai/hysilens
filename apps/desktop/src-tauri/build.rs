//! Tauri host 的构建脚本
//!
//! `tauri_build::build()` 生成 context 代码和 capability manifest，供
//! `tauri::generate_context!` 内嵌，并写入 `OUT_DIR`；该 crate 没有其他构建期输入

fn main() {
    tauri_build::build();
}
