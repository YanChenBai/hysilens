import type { HostStatus } from '@hysilens/plugin/host';
import { defaultSample, samples } from '@hysilens/plugin/samples';
import { Loading, Tabs, TabsContent, TabsList, TabsTrigger, useTheme } from '@hysilens/ui';
import { cn } from 'cn';
import { Bug, Moon, Music, Palette, RotateCw, Sun } from 'kubai-icons-react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { ConfigPanel } from './components/ConfigPanel';
import { IconButton } from './components/IconButton';
import { LogPanel } from './components/LogPanel';
import { LyricsPanel } from './components/LyricsPanel';
import { Stage } from './components/Stage';
import type { StageBackground } from './components/Stage';
import { TransportBar } from './components/TransportBar';
import { fetchPluginInfo, persistConfig, reconcile, restoreConfig, target } from './lib/config';
import type { ConfigValues, PluginInfo } from './lib/config';
import { createTransport, load } from './lib/transport';
import { usePluginHost } from './lib/use-plugin-host';

const PHASE_STYLE: Record<HostStatus['phase'], { label: string; className: string }> = {
  idle: { label: '等待插件', className: 'bg-muted text-muted-foreground' },
  connecting: { label: '连接中', className: 'bg-sky-500/15 text-sky-600 dark:text-sky-400' },
  ready: { label: '就绪', className: 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400' },
  error: { label: '出错', className: 'bg-destructive/15 text-destructive' },
  disposing: { label: '卸载中', className: 'bg-muted text-muted-foreground' },
  disposed: { label: '已卸载', className: 'bg-muted text-muted-foreground' },
};

function Workspace({ info, onReconnect }: { info: PluginInfo; onReconnect: () => void }) {
  const { theme, setTheme } = useTheme();
  const frame = useRef<HTMLIFrameElement>(null);
  const [reloads, setReloads] = useState(0);
  const [background, setBackground] = useState<StageBackground>('checker');
  const [visible, setVisible] = useState(true);
  const [sampleId, setSampleId] = useState(defaultSample.id);
  const [edits, setConfig] = useState<ConfigValues>(() => restoreConfig(info));

  /* 跟随 hysilens.config.ts 修改后的 schema 变化，不需要额外的 effect 轮次 */
  const config = useMemo(() => reconcile(info, edits), [info, edits]);

  const [transport, setTransport] = useState(() =>
    createTransport(defaultSample.durationMs, performance.now()),
  );

  const sample = samples.find(item => item.id === sampleId) ?? defaultSample;

  const lines = useMemo(
    () => sample.document.tracks.find(track => track.kind === 'original')?.lines ?? [],
    [sample],
  );

  const { status, log, clearLog } = usePluginHost(frame, {
    config,
    lyrics: sample.document,
    transport,
    visible,
  });

  useEffect(() => persistConfig(info, config), [info, config]);

  /* 每次 plugin 重载都可能伴随 hysilens.config.ts 修改，需要获取新的 schema */
  useEffect(() => {
    if (status.phase === 'connecting') {
      onReconnect();
    }
  }, [status.phase, onReconnect]);

  const phase = PHASE_STYLE[status.phase];
  const dark = theme !== 'light';
  const scale = info.features.scale && typeof config.scale === 'number' ? config.scale : 1;

  return (
    <div className="bg-background text-foreground flex h-full flex-col">
      <header className="flex shrink-0 items-center gap-3 border-b px-4 py-2">
        <Music color="#f472b6" size={22} />
        <div className="min-w-0">
          <h1 className="truncate text-sm font-semibold">{info.name}</h1>
          <p className="text-muted-foreground truncate text-xs">
            {info.id}
            {info.description ? ` · ${info.description}` : ''}
          </p>
        </div>
        <span
          role="status"
          title={status.error}
          className={cn('rounded-full px-2 py-0.5 text-xs font-medium', phase.className)}
        >
          {phase.label}
        </span>
        {status.error ? (
          <span className="text-destructive min-w-0 truncate text-xs">{status.error}</span>
        ) : null}
        <div className="ml-auto flex items-center gap-1">
          <IconButton label="重新加载插件" onClick={() => setReloads(count => count + 1)}>
            <RotateCw color="#60a5fa" size={20} trigger="parent" />
          </IconButton>
          <IconButton
            label={dark ? '切换到浅色界面' : '切换到深色界面'}
            onClick={() => setTheme(dark ? 'light' : 'dark')}
          >
            {dark ? (
              <Sun color="#fbbf24" size={20} trigger="parent" />
            ) : (
              <Moon color="#818cf8" size={20} trigger="parent" />
            )}
          </IconButton>
        </div>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_360px]">
        <main className="flex min-h-0 flex-col gap-3 p-3">
          <Stage
            frame={frame}
            src={`${target}/__hysilens/plugin?reload=${reloads}`}
            title={info.name}
            viewport={info.viewport ?? { width: 960, height: 540 }}
            pluginScale={scale}
            background={background}
            onBackground={setBackground}
          />
          <TransportBar
            transport={transport}
            onTransport={setTransport}
            lines={lines}
            visible={visible}
            onVisible={setVisible}
          />
        </main>

        <aside className="flex min-h-0 flex-col border-l">
          <Tabs defaultValue="lyrics" className="flex min-h-0 flex-1 flex-col">
            <TabsList className="m-3 shrink-0" aria-label="开发工具面板">
              <TabsTrigger value="lyrics" className="flex-1 gap-1.5">
                <Music color="#f472b6" size={16} />
                歌词
              </TabsTrigger>
              <TabsTrigger value="config" className="flex-1 gap-1.5">
                <Palette color="#a78bfa" size={16} />
                配置
              </TabsTrigger>
              <TabsTrigger value="log" className="flex-1 gap-1.5">
                <Bug color="#f87171" size={16} />
                日志{log.some(entry => entry.level !== 'info') ? ' •' : ''}
              </TabsTrigger>
            </TabsList>
            <TabsContent value="lyrics" className="mt-0 flex min-h-0 flex-1 flex-col">
              <LyricsPanel
                samples={samples}
                sample={sample}
                onSample={id => {
                  const next = samples.find(item => item.id === id) ?? defaultSample;
                  setSampleId(next.id);
                  setTransport(current => load(current, next.durationMs, performance.now()));
                }}
                transport={transport}
                onTransport={setTransport}
              />
            </TabsContent>
            <TabsContent value="config" className="mt-0 flex min-h-0 flex-1 flex-col">
              <ConfigPanel info={info} config={config} onChange={setConfig} />
            </TabsContent>
            <TabsContent value="log" className="mt-0 flex min-h-0 flex-1 flex-col">
              <LogPanel log={log} onClear={clearLog} />
            </TabsContent>
          </Tabs>
        </aside>
      </div>
    </div>
  );
}

export default function App() {
  const [info, setInfo] = useState<PluginInfo | null>(null);
  const [error, setError] = useState('');

  const refresh = useCallback(() => {
    fetchPluginInfo().then(
      next => {
        /* 内容没变时保持同一个对象，避免触发依赖它的状态 */
        setInfo(current => (JSON.stringify(current) === JSON.stringify(next) ? current : next));
        setError('');
      },
      (problem: unknown) => setError(String(problem)),
    );
  }, []);

  useEffect(refresh, [refresh]);

  if (error && !info) {
    return <p className="text-destructive p-6 text-sm">{error}</p>;
  }

  return info ? <Workspace info={info} onReconnect={refresh} /> : <Loading className="h-full" />;
}
