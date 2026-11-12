//! 按版本校验的只读当前队列标识，参见 docs/kugou-matching-research.md
//! 绝不调用目标 vtable：只解码已校验的 getter 并遍历其数据

use super::KugouIdentity;
use anyhow::{Context, ensure};
use serde::Serialize;
use std::collections::HashSet;

const PATTERN: &[i16] = &[
    232, -1, -1, -1, -1, 72, 139, 200, 232, -1, -1, -1, -1, 76, 139, 240, 72, 141, 76, 36, 64, 255,
    21, -1, -1, -1, -1, 144, 72, 141, 77, 128, 232, -1, -1, -1, -1, 144, 232, -1, -1, -1, -1, 72,
    141, 85, 128, 72, 139, 200, 232, -1, -1, -1, -1, 138, 216, 72, 141, 76, 36, 104,
];
const MAX_CODE_BYTES: usize = 64 * 1024 * 1024;
const MAX_TREE_STEPS: usize = 128;

pub(super) trait Memory {
    fn read_into(&self, address: usize, output: &mut [u8]) -> anyhow::Result<()>;
    fn bytes<const N: usize>(&self, address: usize) -> anyhow::Result<[u8; N]> {
        let mut result = [0; N];
        self.read_into(address, &mut result)?;

        Ok(result)
    }
}

impl Memory for super::memory::Reader {
    fn read_into(&self, address: usize, output: &mut [u8]) -> anyhow::Result<()> {
        self.read_into(address, output)
    }
}

fn address(base: usize, offset: usize) -> anyhow::Result<usize> {
    ensure!(base >= 0x10000, "酷狗身份指针为空或无效");

    let address = base.checked_add(offset).context("酷狗身份地址溢出")?;
    ensure!(
        (address as u64) < 0x0000_8000_0000_0000,
        "酷狗身份地址超出用户空间"
    );

    Ok(address)
}

fn pointer(memory: &impl Memory, base: usize, offset: usize) -> anyhow::Result<usize> {
    let value = u64::from_le_bytes(memory.bytes(address(base, offset)?)?);
    let value = usize::try_from(value).context("酷狗身份指针超出本机位宽")?;
    ensure!(
        value == 0 || (0x10000u64..0x0000_8000_0000_0000).contains(&(value as u64)),
        "酷狗身份指针无效"
    );

    Ok(value)
}

fn integer(memory: &impl Memory, base: usize, offset: usize) -> anyhow::Result<i32> {
    Ok(i32::from_le_bytes(memory.bytes(address(base, offset)?)?))
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Entry {
    pub call_rva: usize,

    pub getter_rva: usize,

    pub root_rva: usize,

    #[serde(skip)]
    module_base: usize,

    #[serde(skip)]
    module_size: usize,
}

impl Entry {
    pub fn discover(memory: &impl Memory, base: usize, size: usize) -> anyhow::Result<Self> {
        ensure!(
            size_of::<usize>() == 8 && (4096..=128 * 1024 * 1024).contains(&size),
            "酷狗身份读取需要受支持的 x64 模块"
        );
        ensure!(
            memory.bytes::<2>(base)? == *b"MZ",
            "酷狗身份模块缺少 MZ 标记"
        );

        let nt = u32::from_le_bytes(memory.bytes(address(base, 0x3c)?)?) as usize;
        let within = |offset: usize, length: usize| -> anyhow::Result<usize> {
            ensure!(
                offset.checked_add(length).is_some_and(|end| end <= size),
                "酷狗身份代码地址超出模块"
            );
            address(base, offset)
        };

        let header = memory.bytes::<24>(within(nt, 24)?)?;
        ensure!(
            &header[..4] == b"PE\0\0" && header[4..6] == [0x64, 0x86],
            "酷狗身份模块不是 PE x64"
        );

        let count = u16::from_le_bytes([header[6], header[7]]) as usize;
        let optional_size = u16::from_le_bytes([header[20], header[21]]) as usize;
        ensure!(
            (1..=96).contains(&count) && (0xf0..=0x1000).contains(&optional_size),
            "酷狗身份 PE 节表无效"
        );
        ensure!(
            memory.bytes::<2>(within(nt + 24, 2)?)? == [0x0b, 0x02],
            "酷狗身份模块不是 PE32+"
        );

        let mut hits = Vec::new();
        let mut scanned = 0usize;

        for index in 0..count {
            let section = memory.bytes::<40>(within(nt + 24 + optional_size + index * 40, 40)?)?;
            let field = |offset| {
                u32::from_le_bytes(section[offset..offset + 4].try_into().unwrap_or_default())
                    as usize
            };

            if field(36) & 0x2000_0000 == 0 {
                continue;
            }

            let length = field(8);
            let start = field(12);
            within(start, length)?;
            scanned = scanned
                .checked_add(length)
                .context("酷狗身份扫描长度溢出")?;
            ensure!(scanned <= MAX_CODE_BYTES, "酷狗身份代码扫描超过上限");

            // 只扫描可执行的 module section，每次附着只扫一遍；跨块保留 pattern 首字节，
            // 以便发现横跨读取边界的特征
            let mut carry = Vec::new();
            let mut offset = 0;

            while offset < length {
                let take = (64 * 1024).min(length - offset);
                let mut block = vec![0; carry.len() + take];
                block[..carry.len()].copy_from_slice(&carry);
                memory.read_into(within(start + offset, take)?, &mut block[carry.len()..])?;

                for at in memchr::memchr_iter(PATTERN[0] as u8, &block) {
                    let Some(window) = block.get(at..at + PATTERN.len()) else {
                        continue;
                    };

                    if PATTERN
                        .iter()
                        .zip(window)
                        .all(|(expected, byte)| *expected < 0 || *expected == i16::from(*byte))
                    {
                        hits.push(start + offset - carry.len() + at);
                        ensure!(hits.len() <= 1, "酷狗身份 AOB 多处命中，拒绝猜测入口");
                    }
                }
                carry = block[block.len().saturating_sub(PATTERN.len() - 1)..].to_vec();
                offset += take;
            }
        }

        let call_rva = hits.first().copied().context("酷狗身份 AOB 未命中")?;
        let branch = |rva: usize, opcode| -> anyhow::Result<usize> {
            let code = memory.bytes::<5>(within(rva, 5)?)?;
            ensure!(code[0] == opcode, "酷狗身份跳转指令不匹配");

            let delta = i32::from_le_bytes(code[1..].try_into()?);
            let target = (rva + 5)
                .checked_add_signed(delta as isize)
                .context("酷狗身份跳转目标溢出")?;
            within(target, 5)?;

            Ok(target)
        };

        let thunk = branch(call_rva, 0xe8)?;
        let getter_rva = branch(thunk, 0xe9)?;
        let code = memory.bytes::<13>(within(getter_rva, 13)?)?;

        // 该 getter 以冗余的 REX 前缀 40 53 开头，漏掉它会让 RIP 位移偏移一个字节，
        // 得到错误的 root
        ensure!(
            code[..2] == [0x40, 0x53] && code[6..9] == [0x48, 0x8b, 0x05],
            "酷狗身份单例 getter 不匹配"
        );

        let delta = i32::from_le_bytes(code[9..13].try_into()?);
        let root_rva = (getter_rva + 13)
            .checked_add_signed(delta as isize)
            .context("酷狗身份根槽溢出")?;
        within(root_rva, 8)?;
        Ok(Self {
            call_rva,
            getter_rva,
            root_rva,
            module_base: base,
            module_size: size,
        })
    }

    pub fn read(&self, memory: &impl Memory) -> anyhow::Result<KugouIdentity> {
        // 绝不在采样之间保留堆记录，连续解析两次（含来源引用），使队列或 cache 的变更
        // 让本次采样安全失败
        let first = self.read_once(memory)?;
        ensure!(
            self.read_once(memory)? == first,
            "酷狗身份正在切换，稍后重试"
        );

        Ok(first)
    }

    fn queue(&self, memory: &impl Memory) -> anyhow::Result<(usize, usize)> {
        let manager = pointer(memory, self.module_base, self.root_rva)?;
        let implementation = pointer(memory, manager, 0x20)?;

        Ok((implementation, pointer(memory, implementation, 0x70)?))
    }

    fn read_once(&self, memory: &impl Memory) -> anyhow::Result<KugouIdentity> {
        let (implementation, queue) = self.queue(memory)?;

        // +C8 是状态序列化器当前的 locator；+78 曾被观察到指向上一首歌曲，
        // 不要使用那个旧引擎回退
        let locator = memory.bytes::<16>(address(queue, 0xc8)?)?;
        let item = i32::from_le_bytes(locator[..4].try_into()?);
        let list_id = i32::from_le_bytes(locator[4..8].try_into()?);
        let interface = usize::try_from(u64::from_le_bytes(locator[8..].try_into()?))?;
        ensure!(item > 0 && list_id == 0, "酷狗当前队列类型尚未适配");

        let vtable = pointer(memory, interface, 0)?;
        let getter = pointer(memory, vtable, 8)?;
        ensure!(
            getter >= self.module_base
                && getter
                    .checked_add(6)
                    .is_some_and(|end| end <= self.module_base + self.module_size),
            "酷狗队列类型 getter 越界"
        );
        ensure!(
            memory.bytes::<6>(getter)? == [0xb8, 6, 0, 0, 0, 0xc3],
            "酷狗队列接口类型尚未适配"
        );

        let controller = interface.checked_sub(8).context("酷狗队列接口地址无效")?;
        let depot = pointer(memory, controller, 0x30)?;
        let model = pointer(memory, depot, 0x48)?;
        let owner = pointer(memory, model, 0x38)?;
        let reference = find_intrusive(memory, owner, item, 0x10)?;
        let source_type = integer(memory, reference, 4)?;
        let source_list = integer(memory, reference, 8)?;
        let source_song = integer(memory, reference, 12)?;
        ensure!(
            source_type == 6 && source_list == 0 && source_song > 0,
            "酷狗歌曲来源尚未适配 ({source_type}, {source_list})"
        );

        let provider = pointer(memory, implementation, 0x110)?;
        let source = pointer(memory, provider, 0x40)?;
        let source_vtable = pointer(memory, source, 0)?;

        // AOB 条目之后的偏移专用于 20.1.61.28014；除版本字符串外，
        // 还要校验来源 accessor
        ensure!(
            pointer(memory, source_vtable, 0x138)? == self.module_base + 0xf26a88,
            "酷狗来源歌曲 accessor 不匹配"
        );

        let manager = pointer(memory, source, 0x10)?;
        let lists = pointer(memory, manager, 0x40)?;
        let list = find_list(memory, lists, source_list)?;
        let owner = pointer(memory, list, 0x38)?;
        let record = find_intrusive(memory, owner, source_song, 0xd0)?;
        let standard = memory.bytes::<16>(address(record, 0x38)?)?;
        let played = memory.bytes::<16>(address(record, 0x430)?)?;
        ensure!(
            standard != [0; 16] || played != [0; 16],
            "酷狗当前歌曲没有有效 hash"
        );

        let mix_song_id = u64::from_le_bytes(memory.bytes(address(record, 0x298)?)?);
        ensure!(
            mix_song_id <= 9_007_199_254_740_991,
            "酷狗 MixSongID 超出协议整数范围"
        );

        let recommend_krc_id = u32::from_le_bytes(memory.bytes(address(record, 0x280)?)?);
        ensure!(
            integer(memory, record, 0)? == source_song
                && self.queue(memory)? == (implementation, queue)
                && memory.bytes::<16>(address(queue, 0xc8)?)? == locator,
            "酷狗身份正在切换，稍后重试"
        );
        Ok(KugouIdentity {
            queue_item_id: item,
            source_type,
            list_id: source_list,
            source_song_id: source_song,
            music_file_hash: hash(standard),
            last_played_music_hash: hash(played),
            mix_song_id,
            recommend_krc_id,
        })
    }
}

fn hash(bytes: [u8; 16]) -> String {
    if bytes == [0; 16] {
        return String::new();
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn find_list(memory: &impl Memory, lists: usize, wanted: i32) -> anyhow::Result<usize> {
    let head = pointer(memory, lists, 0x40)?;
    let mut node = pointer(memory, head, 8)?;
    let mut seen = HashSet::new();

    for _ in 0..MAX_TREE_STEPS {
        if node == 0 || node == head {
            break;
        }
        ensure!(seen.insert(node), "酷狗列表树存在循环");

        let nil = memory.bytes::<1>(address(node, 0x19)?)?[0];
        ensure!(nil <= 1, "酷狗列表节点标志无效");

        if nil == 1 {
            break;
        }

        let key = integer(memory, node, 0x20)?;

        if key == wanted {
            return pointer(memory, node, 0x28);
        }
        node = pointer(memory, node, if wanted < key { 0 } else { 0x10 })?;
    }
    anyhow::bail!("酷狗当前来源列表未命中或超过遍历上限")
}

fn find_intrusive(
    memory: &impl Memory,
    owner: usize,
    wanted: i32,
    hook: usize,
) -> anyhow::Result<usize> {
    let head = pointer(memory, owner, 8)?;
    let tagged = pointer(memory, head, hook)? & !1;
    let mut node = if tagged == 0 {
        0
    } else {
        tagged.checked_sub(hook).context("酷狗树根无效")?
    };

    let mut seen = HashSet::new();

    for _ in 0..MAX_TREE_STEPS {
        if node == 0 || node == head {
            break;
        }
        ensure!(seen.insert(node), "酷狗歌曲树存在循环");

        let record = pointer(memory, node, 0)?;
        let key = integer(memory, record, 0)?;

        if key == wanted {
            return Ok(record);
        }

        let child = pointer(memory, node, hook + if wanted < key { 8 } else { 16 })?;
        node = if child == 0 {
            0
        } else {
            child.checked_sub(hook).context("酷狗树子节点无效")?
        };
    }
    anyhow::bail!("酷狗当前条目未命中或超过遍历上限")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const BASE: usize = 0x100000;
    struct Fixture(Vec<u8>);
    impl Memory for Fixture {
        fn read_into(&self, address: usize, output: &mut [u8]) -> anyhow::Result<()> {
            let offset = address.checked_sub(BASE).context("outside fixture")?;
            let data = self
                .0
                .get(offset..offset + output.len())
                .context("outside fixture")?;
            output.copy_from_slice(data);

            Ok(())
        }
    }
    impl Fixture {
        fn put(&mut self, offset: usize, bytes: &[u8]) {
            self.0[offset..offset + bytes.len()].copy_from_slice(bytes);
        }
        fn ptr(&mut self, offset: usize, target: usize) {
            self.put(offset, &((BASE + target) as u64).to_le_bytes());
        }
        fn int(&mut self, offset: usize, value: i32) {
            self.put(offset, &value.to_le_bytes());
        }
        fn branch(&mut self, from: usize, to: usize, opcode: u8) {
            self.put(from, &[opcode]);
            self.int(from + 1, to as i32 - (from + 5) as i32);
        }
    }

    fn code_fixture(call: usize) -> Fixture {
        let mut f = Fixture(vec![0; 0x20000]);
        f.put(0, b"MZ");
        f.int(0x3c, 0x80);
        f.put(0x80, b"PE\0\0");
        f.put(0x84, &[0x64, 0x86, 1, 0]);
        f.put(0x94, &[0xf0, 0]);
        f.put(0x98, &[0x0b, 2]);

        let section = 0x188;
        f.int(section + 8, 0x11000);
        f.int(section + 12, 0x1000);
        f.int(section + 36, 0x20000000);

        let pattern: Vec<_> = PATTERN
            .iter()
            .map(|byte| if *byte < 0 { 0 } else { *byte as u8 })
            .collect();
        f.put(call, &pattern);
        f.branch(call, 0x2000, 0xe8);
        f.branch(0x2000, 0x1800, 0xe9);
        f.put(
            0x1800,
            &[0x40, 0x53, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0x05],
        );
        f.int(0x1809, 0x4000 - 0x180d);
        f
    }

    #[test]
    fn locates_unique_code_across_block_boundary_and_decodes_signed_branches() {
        let f = code_fixture(0x10ff0);
        let entry = Entry::discover(&f, BASE, f.0.len()).unwrap();
        assert_eq!(
            (entry.call_rva, entry.getter_rva, entry.root_rva),
            (0x10ff0, 0x1800, 0x4000)
        );
    }

    #[test]
    fn refuses_missing_ambiguous_or_invalid_code_and_out_of_module_targets() {
        let f = code_fixture(0x3000);
        let mut duplicate = Fixture(f.0.clone());
        duplicate.put(0x4000, &f.0[0x3000..0x3000 + PATTERN.len()]);
        assert!(
            Entry::discover(&duplicate, BASE, duplicate.0.len())
                .unwrap_err()
                .to_string()
                .contains("多处")
        );

        for (offset, bytes) in [
            (0x3000, vec![0]),
            (0x1800, vec![0]),
            (0x3001, i32::MAX.to_le_bytes().to_vec()),
            (0x1809, i32::MAX.to_le_bytes().to_vec()),
        ] {
            let mut bad = Fixture(f.0.clone());
            bad.put(offset, &bytes);
            assert!(Entry::discover(&bad, BASE, bad.0.len()).is_err());
        }
    }

    fn song_fixture() -> (Fixture, Entry) {
        let mut f = Fixture(vec![0; 0x20000]);

        for (at, target) in [
            (0x4000, 0x5000),
            (0x5020, 0x6000),
            (0x6070, 0x7000),
            (0x6110, 0x8000),
            (0x8040, 0x9000),
            (0x9000, 0xa000),
            (0x9010, 0xb000),
            (0xb040, 0xc000),
            (0xc040, 0xd000),
            (0xd008, 0xe000),
            (0xe028, 0xf000),
            (0xf038, 0x10000),
            (0x10008, 0x11000),
            (0x110d0, 0x120d1),
            (0x12000, 0x13000),
            (0x70d0, 0x15008),
            (0x15008, 0x1c000),
            (0x1c008, 0x1d000),
            (0x15030, 0x16000),
            (0x16048, 0x17000),
            (0x17038, 0x18000),
            (0x18008, 0x19000),
            (0x19010, 0x1a011),
            (0x1a000, 0x1b000),
        ] {
            f.ptr(at, target);
        }
        f.int(0x70c8, 42);

        for (offset, value) in [(0, 42), (4, 6), (8, 0), (12, 84)] {
            f.int(0x1b000 + offset, value);
        }
        f.ptr(0xa138, 0xf26a88);
        f.put(0x1d000, &[0xb8, 6, 0, 0, 0, 0xc3]);
        f.int(0x13000, 84);
        f.put(0x13038, &[0xab; 16]);
        f.put(0x13430, &[0xcd; 16]);
        f.int(0x13280, 987);
        f.put(0x13298, &123456u64.to_le_bytes());

        let entry = Entry {
            call_rva: 0,
            getter_rva: 0,
            root_rva: 0x4000,
            module_base: BASE,
            module_size: 0x2000000,
        };
        (f, entry)
    }

    #[test]
    fn reads_distinct_queue_and_source_ids_hash_bytes_and_lyric_version() {
        let (f, entry) = song_fixture();
        let identity = entry.read(&f).unwrap();
        assert_eq!(
            identity,
            KugouIdentity {
                queue_item_id: 42,
                source_type: 6,
                list_id: 0,
                source_song_id: 84,
                music_file_hash: "ab".repeat(16),
                last_played_music_hash: "cd".repeat(16),
                mix_song_id: 123456,
                recommend_krc_id: 987,
            }
        );
    }

    #[test]
    fn rejects_unknown_sources_cycles_empty_hashes_and_missing_nodes() {
        let (f, entry) = song_fixture();

        for (offset, bytes) in [
            (0x1b004, 5i32.to_le_bytes().to_vec()),
            (0x13000, 85i32.to_le_bytes().to_vec()),
            (0x19010, 0u64.to_le_bytes().to_vec()),
            (0x1d001, 7i32.to_le_bytes().to_vec()),
        ] {
            let mut bad = Fixture(f.0.clone());
            bad.put(offset, &bytes);
            assert!(entry.read(&bad).is_err());
        }

        let mut empty = Fixture(f.0.clone());
        empty.put(0x13038, &[0; 16]);
        empty.put(0x13430, &[0; 16]);
        assert!(entry.read(&empty).is_err());

        let mut cycle = Fixture(f.0.clone());
        cycle.int(0x1b000, 0);
        cycle.ptr(0x1a020, 0x1a010);
        assert!(entry.read(&cycle).unwrap_err().to_string().contains("循环"));
    }

    #[test]
    fn rejects_a_locator_that_changes_during_sampling() {
        struct Changing(Fixture, AtomicUsize);
        impl Memory for Changing {
            fn read_into(&self, address: usize, output: &mut [u8]) -> anyhow::Result<()> {
                self.0.read_into(address, output)?;

                if address == BASE + 0x70c8 && self.1.fetch_add(1, Ordering::Relaxed) > 0 {
                    output[0] ^= 1;
                }

                Ok(())
            }
        }

        let (f, entry) = song_fixture();
        assert!(
            entry
                .read(&Changing(f, AtomicUsize::new(0)))
                .unwrap_err()
                .to_string()
                .contains("切换")
        );
    }

    #[test]
    #[ignore = "read-only probe of an already running supported KuGou; never starts the player"]
    fn inspect_running_kugou_identity() {
        let reader = super::super::memory::Reader::attach().unwrap();
        let identity = reader.identity().unwrap();
        println!("{}", serde_json::to_string(&identity).unwrap());

        if let Some(path) = std::env::var_os("HYSILENS_KUGOU_PROBE_CACHE") {
            let catalog = super::super::catalog::read(std::path::Path::new(&path)).unwrap();
            let session = crate::model::Session {
                lyric_match: Some(identity.lyric_match()),
                ..Default::default()
            };

            let result = super::super::catalog::resolve(&catalog, &session, None);
            println!(
                "{}",
                serde_json::json!({"status":result.status,"candidates":result.candidates.len(),"document":result.document.as_ref().map(|doc|&doc.filename),"cacheIssues":catalog.issues.len()})
            );
        }
    }
}
