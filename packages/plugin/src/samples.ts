/**
 * 供预览与 `hysilens dev` 使用的歌词文档
 * 它们合计覆盖 renderer 实际会遇到的各种形态：逐字与逐词计时、副轨、空档、
 * 重叠以及只有整行计时的情形
 */
import type { LyricDocument, LyricLine, LyricToken } from './protocol';

/**
 * 带元数据的示例文档，元数据用于 devtools 选择器展示
 */
export interface LyricSample {
  id: string;

  /**
   * devtools 示例选择器中显示的名称
   */
  name: string;

  /**
   * 该示例覆盖的情形，显示在名称旁边
   */
  description: string;

  document: LyricDocument;

  /**
   * 播放时长，包含最后一行之后的短暂收尾
   */
  durationMs: number;
}

/**
 * `chars`：字素平均分享该行时间；数组形式为按顺序的 `[text, ms]` 词，`''` 表示停顿
 */
type Timing = 'chars' | 'none' | Array<[string, number]>;

/**
 * 示例行，由 `sample` 展开成 token 并合并进三条 track
 */
interface SampleLine {
  /**
   * 在歌词时间线上的起点，单位 ms
   */
  at: number;

  text: string;

  /**
   * 默认为各词时长之和；均匀计时的行则为 4 s
   */
  durationMs?: number;

  /**
   * 默认为 `chars`
   */
  timing?: Timing;

  romanization?: string;

  translation?: string;

  /**
   * 让该行开放式结束，与某些来源的做法一致
   */
  openEnded?: boolean;
}

const segmenter = new Intl.Segmenter(undefined, { granularity: 'grapheme' });

function tokens(line: SampleLine, durationMs: number): LyricToken[] {
  const timing = line.timing ?? 'chars';

  if (timing === 'none') {
    return [];
  }

  if (timing === 'chars') {
    const graphemes = Array.from(segmenter.segment(line.text), ({ segment }) => segment);
    const step = durationMs / Math.max(1, graphemes.length);

    return graphemes.map((text, index) => ({
      text,
      startMs: Math.round(line.at + index * step),
      endMs: Math.round(line.at + (index + 1) * step),
    }));
  }

  let cursor = line.at;
  const result: LyricToken[] = [];

  for (const [text, ms] of timing) {
    if (text) {
      result.push({ text, startMs: cursor, endMs: cursor + ms });
    }

    cursor += ms;
  }

  return result;
}

function duration(line: SampleLine): number {
  if (line.durationMs !== undefined) {
    return line.durationMs;
  }

  return Array.isArray(line.timing) ? line.timing.reduce((sum, [, ms]) => sum + ms, 0) : 4000;
}

function secondary(lines: SampleLine[], key: 'romanization' | 'translation'): LyricLine[] {
  return lines.flatMap(line => {
    const text = line[key];

    return text === undefined
      ? []
      : [{ text, startMs: line.at, endMs: line.at + duration(line), tokens: [] }];
  });
}

function sample(
  id: string,
  name: string,
  description: string,
  metadata: LyricDocument['metadata'],
  lines: SampleLine[],
): LyricSample {
  const original: LyricLine[] = lines.map(line => {
    const length = duration(line);

    return {
      text: line.text,
      startMs: line.at,
      endMs: line.openEnded ? null : line.at + length,
      tokens: line.text ? tokens(line, length) : [],
    };
  });

  const tracks = [{ kind: 'original', lines: original }];
  const romanization = secondary(lines, 'romanization');
  const translation = secondary(lines, 'translation');

  if (romanization.length) {
    tracks.push({ kind: 'romanization', lines: romanization });
  }

  if (translation.length) {
    tracks.push({ kind: 'translation', lines: translation });
  }

  const end = Math.max(...lines.map(line => line.at + duration(line)));

  return {
    id,
    name,
    description,
    durationMs: end + 2000,
    document: { id: `sample:${id}`, filename: `${id}.lrc`, metadata, warnings: [], tracks },
  };
}

const chinese = sample(
  'zh',
  '中文 · 逐字',
  '逐字时间、拼音与英文翻译，长短句交替。',
  { title: '星光练习', artist: 'Hysilens', album: '示例' },
  [
    {
      at: 1000,
      text: '让晚风带走今天的疲惫',
      romanization: 'rang wan feng dai zou jin tian de pi bei',
      translation: 'Let the evening breeze carry your worries',
    },
    {
      at: 6000,
      text: '沿着星光慢慢向前',
      romanization: 'yan zhe xing guang man man xiang qian',
      translation: 'Follow the starlight, step by step',
    },
    {
      at: 11000,
      text: '把心事唱给蓝色的夜',
      romanization: 'ba xin shi chang gei lan se de ye',
      translation: 'Sing your thoughts to the blue night',
    },
    {
      at: 16000,
      text: '下一句仍有新的光',
      durationMs: 3200,
      romanization: 'xia yi ju reng you xin de guang',
      translation: 'There is light in the next line',
    },
    {
      at: 20000,
      text: '让旋律停留在你身旁',
      romanization: 'rang xuan lv ting liu zai ni shen pang',
      translation: 'Let the melody stay beside you',
    },
    { at: 25000, text: '风', durationMs: 1500, romanization: 'feng', translation: 'Wind' },
    {
      at: 27000,
      text: '我们一起等待天亮',
      durationMs: 5000,
      romanization: 'wo men yi qi deng dai tian liang',
      translation: 'Together we wait for the dawn',
    },
  ],
);

const english = sample(
  'en',
  'English · word timing',
  'Uneven word durations, punctuation and held notes.',
  { title: 'Paper Lanterns', artist: 'Hysilens', album: 'Samples' },
  [
    {
      at: 1000,
      text: 'I wait till the evening lights come on,',
      timing: [
        ['I ', 300],
        ['wait ', 450],
        ['till ', 300],
        ['the ', 200],
        ['evening ', 700],
        ['lights ', 600],
        ['come ', 400],
        ['on,', 1200],
      ],
    },
    {
      at: 6000,
      text: 'Remnants on the window, singing along',
      timing: [
        ['Remnants ', 800],
        ['on ', 250],
        ['the ', 200],
        ['window, ', 900],
        ['', 300],
        ['singing ', 800],
        ['along', 1600],
      ],
    },
    { at: 11500, text: 'Ooh—', timing: [['Ooh—', 2500]] },
    {
      at: 14500,
      text: 'Every little light is a song',
      timing: [
        ['Every ', 500],
        ['little ', 450],
        ['light ', 600],
        ['is ', 250],
        ['a ', 200],
        ['song', 2000],
      ],
    },
    {
      at: 19000,
      text: "Don't let go, don't let go",
      timing: [
        ["Don't ", 350],
        ['let ', 300],
        ['go, ', 900],
        ['', 200],
        ["don't ", 350],
        ['let ', 300],
        ['go', 1800],
      ],
    },
  ],
);

const japanese = sample(
  'ja',
  '日本語 · ローマ字',
  '日文逐字、罗马音与中文翻译。',
  { title: '夜明けの歌', artist: 'Hysilens', album: 'サンプル' },
  [
    {
      at: 1000,
      text: '夜明けまで歌おう',
      romanization: 'yoake made utaou',
      translation: '一直唱到黎明',
    },
    {
      at: 5500,
      text: '遠い星を数えて',
      romanization: 'tooi hoshi wo kazoete',
      translation: '数着遥远的星星',
    },
    {
      at: 10000,
      text: 'きみの声が聞こえる',
      romanization: 'kimi no koe ga kikoeru',
      translation: '我听见你的声音',
    },
    {
      at: 14500,
      text: 'ありがとう',
      durationMs: 3000,
      romanization: 'arigatou',
      translation: '谢谢你',
    },
  ],
);

const edgeCases = sample(
  'edge',
  '边界情况',
  '超长句、空行与间奏、无逐字时间、极快逐字、emoji 与组合字符、时间重叠和无结束时间。',
  { title: '边界情况', artist: 'Hysilens', album: '测试' },
  [
    {
      at: 1000,
      text: '这是一句非常非常长的歌词，用来测试换行、裁切和容器宽度的处理是否正确，Mixed with some English words too',
      durationMs: 6000,
      translation: 'A very long line to test wrapping, clipping and container width',
    },
    { at: 7500, text: '', durationMs: 1500 },
    {
      at: 9000,
      text: '只有整行时间，没有逐字',
      timing: 'none',
      romanization: 'zhi you zheng hang shi jian',
    },
    { at: 13000, text: '极快的逐字一二三四五六七八九十', durationMs: 900 },
    { at: 15000, text: '— 间奏 10 秒 —', timing: 'none', durationMs: 1000 },
    {
      at: 26000,
      text: '🌸 花开 é 👨‍👩‍👧 家',
      durationMs: 3500,
      translation: 'Emoji and combining marks',
    },
    { at: 30000, text: '这一句还没唱完', durationMs: 4000 },
    {
      at: 32500,
      text: '下一句已经开始（重叠）',
      durationMs: 3500,
      translation: 'Overlaps the previous line',
    },
    { at: 37000, text: '没有结束时间的最后一句', durationMs: 4000, openEnded: true },
  ],
);

const overlappingLines = sample(
  'overlap',
  '切句重叠 · 密集歌词',
  '中英混排署名、连续短句、尾字跨句与无结束时间；检查新句入场时旧句是否叠在同一位置。',
  { title: '切句重叠排查', artist: 'Hysilens', album: '测试' },
  [
    { at: 1000, text: '混音：Hysilens Music Lab', durationMs: 2200, timing: 'none' },
    { at: 2600, text: '制作：Echo @ VIP MUSIC', durationMs: 2200, timing: 'none' },
    {
      at: 4200,
      text: '雾中散炊烟',
      timing: [
        ['雾中', 500],
        ['散', 300],
        ['炊烟', 1800],
      ],
    },
    { at: 5300, text: '风起', durationMs: 1200 },
    { at: 5600, text: '云散', durationMs: 1200 },
    { at: 5900, text: '回声', durationMs: 1200 },
    { at: 6200, text: '落墨', durationMs: 1200 },
    {
      at: 7600,
      text: '旧句没有结束时间',
      durationMs: 3000,
      openEnded: true,
    },
    { at: 8800, text: '下一句已经入场', durationMs: 1600 },
    { at: 11000, text: '留一段空档看消散', durationMs: 1800 },
    { at: 15000, text: '最后一句缓缓收尾', durationMs: 2400 },
  ],
);

/**
 * 所有示例，按选择器顺序排列；id 唯一并作为选择 key
 */
export const samples: LyricSample[] = [chinese, english, japanese, edgeCases, overlappingLines];

/**
 * devtools 示例选择器中的初始选中项
 */
export const defaultSample = chinese;
