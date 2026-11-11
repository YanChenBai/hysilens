use std::{collections::BTreeSet, sync::OnceLock};

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
