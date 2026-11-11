use crate::model::{Document, Line, LyricTrack, Metadata, Token};
use anyhow::{Context, ensure};
use lyrics_core::{models::flatten_syllable_items, traits::LyricsDecrypter};
use lyrics_crypto::decrypter::qrc::decrypter::QrcDecrypter;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const MAGIC: &[u8] = &[
    0x98, 0x25, 0xb0, 0xac, 0xe3, 0x02, 0x83, 0x68, 0xe8, 0xfc, 0x6c,
];

// 本地 QMC1 预处理改编自 jixunmoe-go/qrc，许可证见 THIRD_PARTY_NOTICES.md。
const QMC_KEY: [u8; 128] = [
    0xc3, 0x4a, 0xd6, 0xca, 0x90, 0x67, 0xf7, 0x52, 0xd8, 0xa1, 0x66, 0x62, 0x9f, 0x5b, 0x09, 0x00,
    0xc3, 0x5e, 0x95, 0x23, 0x9f, 0x13, 0x11, 0x7e, 0xd8, 0x92, 0x3f, 0xbc, 0x90, 0xbb, 0x74, 0x0e,
    0xc3, 0x47, 0x74, 0x3d, 0x90, 0xaa, 0x3f, 0x51, 0xd8, 0xf4, 0x11, 0x84, 0x9f, 0xde, 0x95, 0x1d,
    0xc3, 0xc6, 0x09, 0xd5, 0x9f, 0xfa, 0x66, 0xf9, 0xd8, 0xf0, 0xf7, 0xa0, 0x90, 0xa1, 0xd6, 0xf3,
    0xc3, 0xf3, 0xd6, 0xa1, 0x90, 0xa0, 0xf7, 0xf0, 0xd8, 0xf9, 0x66, 0xfa, 0x9f, 0xd5, 0x09, 0xc6,
    0xc3, 0x1d, 0x95, 0xde, 0x9f, 0x84, 0x11, 0xf4, 0xd8, 0x51, 0x3f, 0xaa, 0x90, 0x3d, 0x74, 0x47,
    0xc3, 0x0e, 0x74, 0xbb, 0x90, 0xbc, 0x3f, 0x92, 0xd8, 0x7e, 0x11, 0x13, 0x9f, 0x23, 0x95, 0x5e,
    0xc3, 0x00, 0x09, 0x5b, 0x9f, 0x62, 0x66, 0xa1, 0xd8, 0x52, 0xf7, 0x67, 0x90, 0xca, 0xd6, 0x4a,
];

pub fn decode(path: &Path) -> anyhow::Result<LyricTrack> {
    parse(&decode_text(path)?, "original")
}

fn decode_text(path: &Path) -> anyhow::Result<String> {
    ensure!(
        fs::metadata(path)?.len() <= 4 * 1024 * 1024,
        "Lyric file exceeds 4 MiB"
    );

    let mut bytes = fs::read(path).with_context(|| format!("Read {}", path.display()))?;

    ensure!(bytes.len() <= 4 * 1024 * 1024, "Lyric file exceeds 4 MiB");

    if bytes.starts_with(MAGIC) {
        for (offset, byte) in bytes.iter_mut().enumerate() {
            let index = if offset > 0x7fff {
                offset % 0x7fff
            } else {
                offset
            };

            *byte ^= QMC_KEY[index & 0x7f];
        }

        bytes.drain(..MAGIC.len());
    }

    let plain = QrcDecrypter
        .decrypt_bytes(&bytes)
        .context("QRC decryption failed")?;

    let plain = String::from_utf8(plain).context("QRC text is not UTF-8")?;

    Ok(plain)
}

fn parse_data(plain: &str) -> anyhow::Result<lyrics_core::models::LyricsData> {
    let xml;

    let body = if plain.trim_start().starts_with('<') {
        xml = lyrics_parsers::xml_utils::create(plain).context("Invalid QRC XML")?;

        lyric_content(&xml).context("QRC XML has no LyricContent")?
    } else {
        plain
    };

    lyrics_parsers::parsers::parse_lyrics_auto(body).context("Unsupported lyric payload")
}

pub fn parse(plain: &str, kind: &str) -> anyhow::Result<LyricTrack> {
    let data = parse_data(plain)?;

    let mut lines: Vec<Line> = data
        .lines
        .unwrap_or_default()
        .into_iter()
        .filter_map(|line| {
            let start_ms = line.start_time()?;

            let end_ms = line.end_time();

            let text = line.text_from_any();

            if start_ms < 0 || text.trim().is_empty() {
                return None;
            }

            let tokens = line
                .syllables()
                .map(flatten_syllable_items)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|token| {
                    let start = token.start_time;

                    let end = token.end_time;

                    (start >= 0 && end >= start).then_some(Token {
                        text: token.text,
                        start_ms: start,
                        end_ms: end,
                    })
                })
                .collect();

            Some(Line {
                start_ms,
                end_ms,
                text,
                tokens,
            })
        })
        .collect();

    lines.sort_by_key(|line| line.start_ms);

    for index in 0..lines.len() {
        if lines[index].end_ms.is_none() {
            lines[index].end_ms = lines
                .iter()
                .skip(index + 1)
                .find(|next| next.start_ms > lines[index].start_ms)
                .map(|next| next.start_ms);
        }
    }

    Ok(LyricTrack {
        kind: kind.into(),
        lines,
    })
}

fn lyric_content(node: &lyrics_parsers::xml_utils::XmlNode) -> Option<&str> {
    node.attribute("LyricContent")
        .or_else(|| node.children.iter().find_map(lyric_content))
}

pub fn load_document(path: &Path) -> anyhow::Result<Document> {
    let filename = path
        .file_name()
        .context("Missing filename")?
        .to_string_lossy()
        .into_owned();

    let plain = decode_text(path)?;

    let data = parse_data(&plain)?;

    let source_metadata = data.track_metadata.unwrap_or_default();

    let metadata = Metadata {
        title: source_metadata.title.unwrap_or_default(),
        artist: source_metadata.artist.unwrap_or_default(),
        album: source_metadata.album.unwrap_or_default(),
    };

    let original = parse(&plain, "original")?;

    let mut tracks = vec![original];

    let mut warnings = Vec::new();

    let base = filename
        .strip_suffix("_qm.qrc")
        .context("Select an original _qm.qrc file")?;

    for (suffix, kind) in [
        ("_qmts.qrc", "translation"),
        ("_qmRoma.qrc", "romanization"),
    ] {
        let companion = path.with_file_name(format!("{base}{suffix}"));

        if !companion.exists() {
            continue;
        }

        match decode(&companion) {
            Ok(mut track) => {
                track.kind = kind.into();

                tracks.push(track);
            }
            Err(error) => warnings.push(format!("{kind}: {error:#}")),
        }
    }

    let digest = Sha256::digest(serde_json::to_vec(&tracks)?);

    Ok(Document {
        id: format!("{digest:x}"),
        filename,
        metadata,
        tracks,
        warnings,
    })
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn preserves_absolute_word_times_line_duration_and_overlap() {
        let track = parse(
            "[1000,2000]A(1000,500)B(1500,300)\n[1000,2500]C(1000,400)",
            "original",
        )
        .unwrap();

        assert_eq!(track.lines.len(), 2);

        assert_eq!(track.lines[0].end_ms, Some(3000));

        assert_eq!(track.lines[0].tokens[1].start_ms, 1500);

        assert_eq!(track.lines[1].start_ms, 1000);
    }

    #[test]
    fn accepts_xml_envelope_and_line_synced_translation() {
        let track = parse(
            "<Qrc><LyricInfo LyricContent=\"[00:01.00]示例&#10;[00:02.00]下一句\"/></Qrc>",
            "translation",
        )
        .unwrap();

        assert_eq!(track.lines[0].text, "示例");

        assert_eq!(track.lines[0].end_ms, Some(2000));

        assert!(track.lines[0].tokens.is_empty());

        assert!(parse("<Qrc/>", "original").is_err());
    }

    #[test]
    fn keeps_literal_newlines_in_local_qrc_attributes() {
        // 通用 XML DOM 会把属性中的换行归一为空格，导致整首 QRC 无法分行。
        let track = parse("<Qrc><Lyric_1 LyricContent=\"[ti:示例]\n[1000,500]A(1000,500)\n[2000,500]B(2000,500)\"/></Qrc>", "original").unwrap();

        assert_eq!(track.lines.len(), 2);

        assert_eq!(track.lines[1].start_ms, 2000);
    }
}
