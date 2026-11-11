use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OverlaySettings {
    pub color: String,
    pub active_color: String,
    pub background: String,
    pub font_family: String,
    pub font_size: f64,
    pub line_gap: f64,
    pub visible_lines: u8,
    pub offset: f64,
    pub romanization: bool,
    pub translation: bool,
    pub renderer: String,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        // Rust 服务和前端共用一份默认值，避免两端主题漂移。
        let value: serde_json::Value =
            serde_json::from_str(include_str!("../../../config/overlay-defaults.json"))
                .expect("Valid built-in overlay defaults");
        Self {
            color: value["color"].as_str().unwrap().into(),
            active_color: value["activeColor"].as_str().unwrap().into(),
            background: value["background"].as_str().unwrap().into(),
            font_family: value["fontFamily"].as_str().unwrap().into(),
            font_size: value["fontSize"].as_f64().unwrap(),
            line_gap: value["lineGap"].as_f64().unwrap(),
            visible_lines: value["visibleLines"].as_u64().unwrap() as u8,
            offset: value["offset"].as_f64().unwrap(),
            romanization: value["romanization"].as_bool().unwrap(),
            translation: value["translation"].as_bool().unwrap(),
            renderer: value["renderer"].as_str().unwrap().into(),
        }
    }
}

impl OverlaySettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(crate::plugins::valid_id(&self.renderer), "歌词插件标识无效");
        anyhow::ensure!(
            matches!(self.visible_lines, 2 | 3),
            "滚动歌词行数需为 2 或 3"
        );
        anyhow::ensure!(
            is_color(&self.color) && is_color(&self.active_color),
            "歌词颜色需为 #RRGGBB 或 #RRGGBBAA"
        );
        anyhow::ensure!(
            self.background == "transparent" || is_color(&self.background),
            "背景颜色格式无效"
        );
        anyhow::ensure!(
            !self.font_family.is_empty()
                && self.font_family.chars().count() <= 80
                && self
                    .font_family
                    .chars()
                    .all(|character| character.is_alphanumeric() || " _-".contains(character)),
            "字体名称无效"
        );
        anyhow::ensure!(
            self.font_size.is_finite() && (14.0..=96.0).contains(&self.font_size),
            "字号范围为 14–96"
        );
        anyhow::ensure!(
            self.line_gap.is_finite() && (0.0..=48.0).contains(&self.line_gap),
            "行间距范围为 0–48"
        );
        anyhow::ensure!(
            self.offset.is_finite() && (-10000.0..=10000.0).contains(&self.offset),
            "歌词偏移范围为 -10000–10000 ms"
        );
        Ok(())
    }
}

fn is_color(value: &str) -> bool {
    (value.len() == 7 || value.len() == 9)
        && value.starts_with('#')
        && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_shared_defaults_and_rejects_invalid_styles() {
        let mut settings = OverlaySettings::default();
        settings.validate().unwrap();
        settings.color = "red;display:none".into();
        assert!(settings.validate().is_err());
        settings = OverlaySettings::default();
        settings.font_size = f64::NAN;
        assert!(settings.validate().is_err());
        let decoded: OverlaySettings = serde_json::from_str("{}").unwrap();
        assert_eq!(decoded, OverlaySettings::default());
    }

    #[test]
    fn accepts_two_or_three_lines_and_defaults_older_configuration() {
        let mut settings = OverlaySettings::default();
        assert_eq!(settings.visible_lines, 3);

        for count in [2, 3] {
            settings.visible_lines = count;
            settings.validate().unwrap();
            let encoded = serde_json::to_string(&settings).unwrap();
            let decoded: OverlaySettings = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded.visible_lines, count);
        }

        for count in [0, 1, 4] {
            settings.visible_lines = count;
            assert!(settings.validate().is_err());
        }
    }
}
