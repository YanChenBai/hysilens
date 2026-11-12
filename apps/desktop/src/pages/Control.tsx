import { parseOverlayConfig } from '@hysilens/protocol/config';
import {
  Button,
  FieldError,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  ThemeProvider,
  TooltipProvider,
  useTheme,
} from '@hysilens/ui';
import { Monitor, Moon, Music, Palette, Settings, Sun } from 'kubai-icons-react';
import { useRef, useState } from 'react';
import type { CSSProperties } from 'react';

import { FooterActionsContext } from '../components/FooterActions';
import { NowPlaying } from '../components/NowPlaying';
import { PlaybackPanel } from '../components/PlaybackPanel';
import { PluginPanel } from '../components/PluginPanel';
import { PreviewPanel } from '../components/PreviewPanel';
import { SettingsPanel } from '../components/SettingsPanel';
import { SettingsRow, SettingsSection } from '../components/SettingsSection';
import { useSession } from '../lib/session';

/**
 * 浅色/深色/跟随系统开关，通过共享的 theme provider 立即生效
 */
function ThemePicker() {
  const { theme, setTheme } = useTheme();

  return (
    <div className="flex items-center gap-0.5" aria-label="界面主题">
      {(
        [
          { value: 'light', Icon: Sun, label: '浅色主题', color: '#f5b83d' },
          { value: 'dark', Icon: Moon, label: '深色主题', color: '#a78bfa' },
          { value: 'system', Icon: Monitor, label: '跟随系统', color: '#60a5fa' },
        ] as const
      ).map(({ value, Icon, label, color }) => (
        <Button
          key={value}
          size="icon"
          variant={theme === value ? 'secondary' : 'ghost'}
          className="size-8"
          aria-label={label}
          aria-pressed={theme === value}
          onClick={() => setTheme(value)}
        >
          <Icon className="size-4" color={color} trigger="parent" />
        </Button>
      ))}
    </div>
  );
}

/**
 * 控制工作台：preview 区域、可拖拽分隔条与设置标签页
 */
function Workspace() {
  const { snapshot, connection, error, clockOffset } = useSession();
  const overlay = parseOverlayConfig(snapshot?.overlay).options;
  const failure = error || snapshot?.session?.error;
  const connected = connection === 'connected';
  const workspace = useRef<HTMLDivElement>(null);
  const [previewWidth, setPreviewWidth] = useState(60);
  const [dragging, setDragging] = useState(false);
  const [footerActions, setFooterActions] = useState<HTMLDivElement | null>(null);

  /**
   * 以百分比设置 preview 宽度，并限制 preview 至少保留 280 px、设置列至少保留 340 px
   */
  function resize(percent: number) {
    const width = workspace.current?.clientWidth;

    if (!width) {
      return;
    }

    const minimum = (280 / width) * 100;
    const maximum = ((width - 340) / width) * 100;
    setPreviewWidth(Math.min(maximum, Math.max(minimum, percent)));
  }

  return (
    <FooterActionsContext value={footerActions}>
      <main className="h-dvh min-h-0">
        <h1 className="sr-only">歌词工作台</h1>
        <div
          ref={workspace}
          className={`control-workspace grid h-full min-h-0 ${dragging ? 'cursor-col-resize select-none [&_iframe]:pointer-events-none' : ''}`}
          style={{ '--preview-width': `${previewWidth}%` } as CSSProperties}
        >
          <section id="lyrics-preview" className="h-full min-h-0 min-w-0" aria-label="歌词画面">
            <PreviewPanel active={overlay.renderer} />
          </section>
          <div
            role="separator"
            tabIndex={0}
            aria-label="调整歌词预览宽度"
            aria-orientation="vertical"
            aria-controls="lyrics-preview lyrics-settings"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(previewWidth)}
            aria-valuetext={`预览宽度 ${Math.round(previewWidth)}%`}
            className="group relative z-20 hidden w-0 cursor-col-resize touch-none outline-none md:block"
            onPointerDown={event => {
              if (event.button !== 0) {
                return;
              }

              event.preventDefault();
              event.currentTarget.focus();
              event.currentTarget.setPointerCapture(event.pointerId);
              setDragging(true);
            }}
            onPointerMove={event => {
              if (!event.currentTarget.hasPointerCapture(event.pointerId)) {
                return;
              }

              const bounds = workspace.current?.getBoundingClientRect();

              if (bounds) {
                resize(((event.clientX - bounds.left) / bounds.width) * 100);
              }
            }}
            onPointerUp={event => {
              if (event.currentTarget.hasPointerCapture(event.pointerId)) {
                event.currentTarget.releasePointerCapture(event.pointerId);
              }

              setDragging(false);
            }}
            onLostPointerCapture={() => setDragging(false)}
            onPointerCancel={() => setDragging(false)}
            onDoubleClick={() => resize(60)}
            onKeyDown={event => {
              const adjustment = event.shiftKey ? 10 : 2;

              const next = {
                ArrowLeft: previewWidth - adjustment,
                ArrowRight: previewWidth + adjustment,
                Home: 0,
                End: 100,
                Enter: 60,
              }[event.key];

              if (next !== undefined) {
                event.preventDefault();
                resize(next);
              }
            }}
          >
            <span className="absolute inset-y-0 -left-1.5 flex w-3 items-center justify-center">
              <span
                className={`group-hover:bg-primary/60 group-focus-visible:bg-primary/60 h-10 w-1 rounded-full transition-colors ${dragging ? 'bg-primary/60' : 'bg-muted-foreground/50'}`}
              />
            </span>
          </div>
          <section
            id="lyrics-settings"
            className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden border-t md:border-t-0 md:border-l"
            aria-label="歌词配置"
          >
            <Tabs defaultValue="appearance" className="flex min-h-0 flex-1 flex-col">
              <TabsList
                className="mt-4 shrink-0 justify-start gap-2 rounded-none bg-transparent px-5 py-0 shadow-[inset_0_-1px_0_var(--color-border)]"
                indicatorClassName="top-auto bottom-0 h-0.5 translate-y-0 rounded-none bg-foreground shadow-none"
                aria-label="歌词设置"
              >
                <TabsTrigger
                  value="appearance"
                  className="min-w-0 gap-1 rounded-none px-2 pt-2 pb-4"
                >
                  <Palette color="#a78bfa" trigger="parent" />
                  插件配置
                </TabsTrigger>
                <TabsTrigger value="playback" className="min-w-0 gap-1 rounded-none px-2 pt-2 pb-4">
                  <Music color="#34d399" trigger="parent" />
                  播放设置
                </TabsTrigger>
                <TabsTrigger value="settings" className="min-w-0 gap-1 rounded-none px-2 pt-2 pb-4">
                  <Settings color="#60a5fa" trigger="parent" />
                  系统设置
                </TabsTrigger>
              </TabsList>
              <div className="control-tab-panels grid min-h-0 flex-1 overflow-hidden">
                <TabsContent value="appearance" className="mt-0 h-full">
                  <PluginPanel active={overlay.renderer} />
                </TabsContent>
                <TabsContent
                  value="playback"
                  className="mt-0 h-full overflow-y-auto px-5 pt-5 pb-6"
                >
                  <PlaybackPanel />
                </TabsContent>
                <TabsContent
                  value="settings"
                  className="mt-0 h-full overflow-y-auto px-5 pt-5 pb-6"
                >
                  <div className="space-y-7">
                    <SettingsSection title="外观" icon={<Palette size={20} color="#a78bfa" />}>
                      <SettingsRow label="界面主题" description="即时生效">
                        <ThemePicker />
                      </SettingsRow>
                    </SettingsSection>
                    <SettingsPanel />
                  </div>
                </TabsContent>
              </div>
            </Tabs>
            <footer className="bg-background shrink-0 space-y-2 border-t px-4 py-3">
              <FieldError show={Boolean(failure)}>{failure}</FieldError>
              <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-2">
                <NowPlaying snapshot={snapshot} connected={connected} clockOffset={clockOffset} />
                <div className="ml-auto flex shrink-0 items-center gap-2">
                  <div ref={setFooterActions} className="flex items-center gap-2 empty:hidden" />
                </div>
              </div>
            </footer>
          </section>
        </div>
      </main>
    </FooterActionsContext>
  );
}

/**
 * 控制页入口，提供各面板依赖的 theme 与 tooltip provider
 */
export default function Control() {
  return (
    <ThemeProvider storageKey="hysilens-next-theme" defaultTheme="dark">
      <TooltipProvider>
        <Workspace />
      </TooltipProvider>
    </ThemeProvider>
  );
}
