//! settings UI 中提供的已安装字体族

use std::{collections::BTreeSet, sync::OnceLock};

/// 系统中已安装的字体族名称，已排序并去掉以 `@` 开头的竖排别名
///
/// 字体数据库每个进程只扫描一次；后续调用返回缓存后的列表
pub fn system_families() -> &'static [String] {
    static FAMILIES: OnceLock<Vec<String>> = OnceLock::new();

    FAMILIES.get_or_init(|| {
        let mut database = fontdb::Database::new();
        database.load_system_fonts();

        database
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .filter(|name| !name.starts_with('@'))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    })
}
