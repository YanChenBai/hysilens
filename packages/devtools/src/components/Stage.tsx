import type { PluginViewport } from '@hysilens/plugin';
import { Grid, Moon, Sun } from 'kubai-icons-react';
import { useEffect, useRef, useState } from 'react';
import type { RefObject } from 'react';

import { IconButton } from './IconButton';

/**
 * plugin 背后的棋盘格或纯色背景，模拟 OBS 场景背景
 */
export type StageBackground = 'checker' | 'dark' | 'light';

interface StageProps {
  frame: RefObject<HTMLIFrameElement | null>;

  src: string;

  title: string;

  /**
   * plugin 声明的 viewport，单位为 CSS 像素
   */
  viewport: PluginViewport;

  /**
   * host 侧的 `scale` feature 值，像真实 host 一样叠加在适配缩放之上
   */
  pluginScale: number;

  background: StageBackground;

  onBackground: (value: StageBackground) => void;
}

const BACKGROUNDS: Array<{
  value: StageBackground;

  label: string;

  icon: typeof Grid;

  color: string;
}> = [
  { value: 'checker', label: '透明（OBS 合成效果）', icon: Grid, color: '#94a3b8' },
  { value: 'dark', label: '深色背景', icon: Moon, color: '#818cf8' },
  { value: 'light', label: '浅色背景', icon: Sun, color: '#fbbf24' },
];

function useSize(element: RefObject<HTMLElement | null>) {
  const [size, setSize] = useState({ width: 0, height: 0 });

  useEffect(() => {
    const target = element.current;

    if (!target) {
      return;
    }

    const observer = new ResizeObserver(([entry]) => {
      if (entry) {
        setSize({ width: entry.contentRect.width, height: entry.contentRect.height });
      }
    });

    observer.observe(target);

    return () => observer.disconnect();
  }, [element]);

  return size;
}

/**
 * 按声明 viewport 渲染并缩放适配后的 plugin，效果类似 OBS 合成
 */
export function Stage({
  frame,
  src,
  title,
  viewport,
  pluginScale,
  background,
  onBackground,
}: StageProps) {
  const area = useRef<HTMLDivElement>(null);
  const size = useSize(area);
  const padding = 48;

  const fit = Math.min(
    1,
    (size.width - padding) / viewport.width,
    (size.height - padding) / viewport.height,
  );

  const zoom = Math.max(0.05, fit);

  return (
    <section className="relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border">
      <div
        ref={area}
        className={`stage-${background} relative grid min-h-0 flex-1 place-items-center`}
      >
        <div
          className="relative overflow-hidden outline-1 outline-white/20 outline-dashed"
          style={{ width: viewport.width * zoom, height: viewport.height * zoom }}
        >
          {/* 与 host 的 canvas 缩放一致：居中并裁剪到 viewport */}
          <div className="absolute inset-0" style={{ transform: `scale(${pluginScale})` }}>
            <iframe
              ref={frame}
              src={src}
              title={title}
              sandbox="allow-scripts"
              className="absolute top-0 left-0 origin-top-left border-0 bg-transparent"
              style={{
                width: viewport.width,
                height: viewport.height,
                transform: `scale(${zoom})`,

                /*
                 * 必须与 plugin 页面的配色方案一致，否则浏览器会在 frame 背后
                 * 绘制不透明画布，导致透明失效
                 */
                colorScheme: 'light',
              }}
            />
          </div>
        </div>
      </div>
      <div className="bg-background/80 absolute top-3 right-3 flex gap-1 rounded-xl border p-1 backdrop-blur">
        {BACKGROUNDS.map(({ value, label, icon: Icon, color }) => (
          <IconButton
            key={value}
            label={label}
            pressed={background === value}
            onClick={() => onBackground(value)}
          >
            <Icon color={color} size={18} trigger="parent" />
          </IconButton>
        ))}
      </div>
      <p className="bg-background/80 text-muted-foreground absolute bottom-3 left-3 rounded-lg px-2 py-1 text-xs tabular-nums backdrop-blur">
        {viewport.width} × {viewport.height} · {Math.round(zoom * 100)}%
        {pluginScale !== 1 ? ` · 插件缩放 ${pluginScale}` : ''}
      </p>
    </section>
  );
}
