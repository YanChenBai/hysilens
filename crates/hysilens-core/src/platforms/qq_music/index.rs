//! QQ Music cache index 文件的离线检查
//!
//! 用于逆向 cache 格式的阶段，播放时不会执行这里的代码

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs, path::PathBuf};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    path: String,

    bytes: usize,

    sha256: String,

    header_hex: String,

    entropy_bits_per_byte: f64,

    duplicate_eight_byte_blocks: usize,

    same_eight_byte_blocks_as_previous: Option<usize>,

    common_prefix_bytes: Option<usize>,
}

/// 打印描述 `paths` 中每个文件的 JSON 报告
///
/// 每个文件最多读取 64 MiB，报告会把每个文件与前一个文件比较
/// （共享的八字节块和公共前缀），以暴露重复 header 或零填充
///
/// # Errors
///
/// 文件无法读取或报告无法序列化时返回错误
pub fn inspect(paths: &[PathBuf]) -> anyhow::Result<()> {
    let mut reports = Vec::new();
    let mut previous: Option<Vec<u8>> = None;

    for path in paths {
        anyhow::ensure!(
            fs::metadata(path)?.len() <= 64 * 1024 * 1024,
            "Index file exceeds 64 MiB"
        );

        let bytes = fs::read(path)?;
        let mut counts = [0_usize; 256];

        for byte in &bytes {
            counts[*byte as usize] += 1;
        }

        let entropy = counts
            .iter()
            .filter(|count| **count > 0)
            .map(|count| {
                let probability = *count as f64 / bytes.len() as f64;

                -probability * probability.log2()
            })
            .sum();

        let mut blocks = HashSet::new();
        let duplicate_blocks = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .filter(|block| !blocks.insert(*block))
            .count();

        reports.push(Report {
            path: path.display().to_string(),
            bytes: bytes.len(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            header_hex: bytes
                .iter()
                .take(16)
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(" "),
            entropy_bits_per_byte: entropy,
            duplicate_eight_byte_blocks: duplicate_blocks,
            same_eight_byte_blocks_as_previous: previous.as_ref().map(|old| {
                old.as_chunks::<8>()
                    .0
                    .iter()
                    .zip(bytes.as_chunks::<8>().0.iter())
                    .filter(|(left, right)| left == right)
                    .count()
            }),
            common_prefix_bytes: previous.as_ref().map(|old| {
                old.iter()
                    .zip(&bytes)
                    .take_while(|(left, right)| left == right)
                    .count()
            }),
        });

        previous = Some(bytes);
    }

    println!("{}", serde_json::to_string_pretty(&reports)?);

    Ok(())
}
