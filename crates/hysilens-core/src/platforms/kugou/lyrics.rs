use crate::model::{Document, LyricTrack, Metadata};
use anyhow::{Context, ensure};
use lyrics_core::traits::LyricsDecrypter;
use lyrics_crypto::decrypter::krc::decrypter::KrcDecrypter;
use lyrics_parsers::parsers::krc_parser;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

pub fn load_document(path: &Path) -> anyhow::Result<Document> {
    ensure!(
        fs::metadata(path)?.len() <= 4 * 1024 * 1024,
        "KRC 文件超过 4 MiB"
    );
    let bytes = fs::read(path)?;
    ensure!(
        bytes.len() <= 4 * 1024 * 1024 && bytes.starts_with(b"krc1"),
        "KRC 文件头无效"
    );
    let plain = String::from_utf8(KrcDecrypter.decrypt_bytes(&bytes)?)?;
    parse(
        plain.trim_start_matches('\u{feff}'),
        path.file_name()
            .context("缺少文件名")?
            .to_string_lossy()
            .into_owned(),
    )
}

fn parse(plain: &str, filename: String) -> anyhow::Result<Document> {
    let original = super::super::qq_music::lyrics::parse(plain, "original")?;
    ensure!(!original.lines.is_empty(), "KRC 没有同步歌词");
    let metadata = krc_parser::parse(plain).track_metadata.unwrap_or_default();
    let mut tracks = vec![original];
    let mut warnings = Vec::new();
    if let Some(content) =
        krc_parser::get_translation_raw_from_krc(plain).and_then(|value| value.content)
    {
        for item in content {
            let kind = match item.content_type {
                0 => "romanization",
                1 => "translation",
                _ => continue,
            };
            let Some(rows) = item.lyric_content else {
                continue;
            };
            // 附属轨道按原始行序对齐；空行也占位，不可先过滤后再 zip。
            let data = krc_parser::parse(plain);
            let source_lines = data.lines.unwrap_or_default();
            if rows.len() != source_lines.len() {
                warnings.push(format!("{kind} 行数不一致，已跳过"));
                continue;
            }
            let mut lines = Vec::new();
            for (source, row) in source_lines.iter().zip(rows) {
                let Some(row) = row else { continue };
                let Some(start_ms) = source.start_time() else {
                    continue;
                };
                let text = if kind == "romanization" {
                    row.join(" ")
                } else {
                    row.first().cloned().unwrap_or_default()
                };
                if text.trim().is_empty() || text == "//" {
                    continue;
                }
                let mut tokens = Vec::new();
                if kind == "romanization" {
                    let syllables = source
                        .syllables()
                        .map(lyrics_core::models::flatten_syllable_items)
                        .unwrap_or_default();
                    if row.len() == syllables.len() {
                        for (index, (text, syllable)) in row.iter().zip(syllables).enumerate() {
                            tokens.push(crate::model::Token {
                                text: format!("{}{}", if index == 0 { "" } else { " " }, text),
                                start_ms: syllable.start_time,
                                end_ms: syllable.end_time,
                            });
                        }
                    }
                }
                lines.push(crate::model::Line {
                    start_ms,
                    end_ms: source.end_time(),
                    text,
                    tokens,
                });
            }
            lines.sort_by_key(|line| line.start_ms);
            if !lines.is_empty() {
                tracks.push(LyricTrack {
                    kind: kind.into(),
                    lines,
                });
            }
        }
    }
    let id = format!("{:x}", Sha256::digest(serde_json::to_vec(&tracks)?));
    Ok(Document {
        id,
        filename,
        metadata: Metadata {
            title: metadata.title.unwrap_or_default(),
            artist: metadata.artist.unwrap_or_default(),
            album: metadata.album.unwrap_or_default(),
        },
        tracks,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligns_embedded_translation_and_romanization_with_original_words() {
        let document = parse("[language:eyJjb250ZW50IjpbeyJ0eXBlIjowLCJseXJpY0NvbnRlbnQiOltbIm5pIiwiaGFvIl1dfSx7InR5cGUiOjEsImx5cmljQ29udGVudCI6W1siSGVsbG8iXV19XX0=]\n[1000,2000]<0,300,0>你<300,500,0>好", "sample.krc".into()).unwrap();
        assert_eq!(document.tracks.len(), 3);
        assert_eq!(document.tracks[1].kind, "romanization");
        assert_eq!(document.tracks[1].lines[0].text, "ni hao");
        assert_eq!(document.tracks[1].lines[0].tokens[1].start_ms, 1300);
        assert_eq!(document.tracks[2].lines[0].text, "Hello");
        assert_eq!(document.tracks[2].lines[0].start_ms, 1000);
    }

    #[test]
    fn preserves_relative_word_times_and_line_duration() {
        let document = parse(
            "[ti:示例]\n[ar:歌手]\n[1000,2000]<0,300,0>你<300,500,0>好",
            "sample.krc".into(),
        )
        .unwrap();
        assert_eq!(document.metadata.title, "示例");
        let line = &document.tracks[0].lines[0];
        assert_eq!(line.start_ms, 1000);
        assert_eq!(line.end_ms, Some(3000));
        assert_eq!(line.tokens[1].start_ms, 1300);
        assert_eq!(line.tokens[1].end_ms, 1800);
    }
}
