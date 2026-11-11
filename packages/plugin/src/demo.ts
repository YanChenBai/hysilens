import type { LyricDocument } from './protocol';

/** Six lines at a fixed cadence; previews loop over `DEMO_DURATION_MS`. */
const LINE_MS = 6000;

export const DEMO_DURATION_MS = 6 * LINE_MS;

const segmenter = new Intl.Segmenter('zh', { granularity: 'grapheme' });

const lines = [
  '让晚风带走今天的疲惫',
  '沿着星光慢慢向前',
  '把心事唱给蓝色的夜',
  '下一句仍有新的光',
  '让旋律停留在你身旁',
  '我们一起等待天亮',
];

const phonetics = [
  'rang wan feng dai zou jin tian de pi bei',
  'yan zhe xing guang man man xiang qian',
  'ba xin shi chang gei lan se de ye',
  'xia yi ju reng you xin de guang',
  'rang xuan lv ting liu zai ni shen pang',
  'wo men yi qi deng dai tian liang',
];

const translations = [
  'Let the evening breeze carry your worries',
  'Follow the starlight, step by step',
  'Sing your thoughts to the blue night',
  'There is light in the next line',
  'Let the melody stay beside you',
  'Together we wait for the dawn',
];

function secondary(kind: string, texts: string[]) {
  return {
    kind,
    lines: texts.map((text, index) => ({
      text,
      startMs: index * LINE_MS,
      endMs: index * LINE_MS + 4800,
      tokens: [],
    })),
  };
}

export const demo: LyricDocument = {
  id: 'playground',
  filename: 'playground',
  metadata: { title: '星光练习', artist: 'Hysilens', album: '' },
  warnings: [],
  tracks: [
    {
      kind: 'original',
      lines: lines.map((text, index) => ({
        startMs: index * LINE_MS,
        endMs: index * LINE_MS + 4800,
        text,
        tokens: Array.from(segmenter.segment(text), ({ segment }) => segment).map(
          (segment, token) => ({
            text: segment,
            startMs: index * LINE_MS + token * 400,
            endMs: index * LINE_MS + (token + 1) * 400,
          }),
        ),
      })),
    },
    secondary('romanization', phonetics),
    secondary('translation', translations),
  ],
};
