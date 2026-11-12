import { Button, useTheme } from '@hysilens/ui';
import { useQuery } from '@tanstack/react-query';
import { Monitor, Play, Radio } from 'kubai-icons-react';
import { useEffect, useRef, useState } from 'react';

import { getJson } from '../lib/api';
import type { PluginCatalog } from '../lib/api';
import { CopySourceButton } from './CopySourceButton';

/**
 * 告诉 preview iframe 是否应使用演示数据计时
 */
function postMode(frame: HTMLIFrameElement | null, playground: boolean) {
  frame?.contentWindow?.postMessage({ kind: 'hysilens:preview-mode', playground }, location.origin);
}

/**
 * 当前 plugin 的实时 iframe preview，带演示/实时切换与 OBS 源地址
 *
 * @param active renderer id，其声明的 viewport 在有效时显示为推荐尺寸
 */
export function PreviewPanel({ active }: { active: string }) {
  const [demo, setDemo] = useState(true);
  const { theme } = useTheme();
  const frame = useRef<HTMLIFrameElement>(null);

  const catalog = useQuery({
    queryKey: ['plugins'],
    queryFn: () => getJson<PluginCatalog>('/api/plugins'),
  });

  const viewport = catalog.data?.plugins.find(plugin => plugin.id === active)?.viewport;

  const hasViewport =
    viewport &&
    Number.isInteger(viewport.width) &&
    viewport.width > 0 &&
    Number.isInteger(viewport.height) &&
    viewport.height > 0;

  useEffect(() => {
    function receive(event: MessageEvent) {
      if (
        event.origin === location.origin &&
        event.source === frame.current?.contentWindow &&
        event.data?.kind === 'hysilens:preview-ready'
      ) {
        postMode(frame.current, demo);
      }
    }

    window.addEventListener('message', receive);
    postMode(frame.current, demo);

    return () => window.removeEventListener('message', receive);
  }, [demo, theme]);

  return (
    <div className="text-foreground relative h-full min-h-0 overflow-hidden bg-[#fafafa] dark:bg-black">
      <iframe
        ref={frame}
        title="歌词画面预览"
        src={`/playground?preview=${theme}`}
        onLoad={() => postMode(frame.current, demo)}
        style={{ colorScheme: theme === 'system' ? 'light dark' : theme }}
        className="absolute inset-0 h-full w-full border-0 bg-transparent"
      />
      <div className="absolute inset-x-0 top-0 z-10 flex items-center justify-between gap-4 bg-gradient-to-b from-[#fafafa]/90 to-transparent px-5 pt-4 pb-10 dark:from-black/80">
        <h2 className="flex items-center gap-2 text-sm font-medium">
          <Monitor size={20} color="#60a5fa" />
          歌词预览
        </h2>
        <div className="flex gap-1">
          <Button
            variant={demo ? 'secondary' : 'ghost'}
            size="sm"
            aria-pressed={demo}
            onClick={() => setDemo(true)}
          >
            <Play color="#a78bfa" trigger="parent" />
            演示
          </Button>
          <Button
            variant={!demo ? 'secondary' : 'ghost'}
            size="sm"
            aria-pressed={!demo}
            onClick={() => setDemo(false)}
          >
            <Radio color="#34d399" trigger="parent" />
            实时
          </Button>
        </div>
      </div>
      <div className="bg-background/80 absolute right-4 bottom-4 z-10 flex max-w-[calc(100%-2rem)] flex-wrap items-center justify-end gap-x-3 rounded-lg px-2 backdrop-blur-sm">
        {hasViewport ? (
          <span className="text-muted-foreground py-2 text-xs whitespace-nowrap tabular-nums">
            建议 {viewport.width} × {viewport.height}
          </span>
        ) : null}
        <CopySourceButton url={`${location.origin}/`} />
      </div>
    </div>
  );
}
