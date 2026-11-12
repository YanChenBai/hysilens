import type { LyricDocument, Session } from '@hysilens/protocol';
import { parseOverlayConfig } from '@hysilens/protocol/config';
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
  FieldError,
} from '@hysilens/ui';
import { useState } from 'react';

import { lyricLineId, lyricSongId } from '../lib/ignored-lyrics';
import { readSession, sendOverlay } from '../lib/session';

/**
 * 打开按歌曲划分的列表、用于从 overlay 中移除歌词行的按钮
 *
 * @param document 当前歌词 document；变化时会重新挂载选择对话框
 * @param session 用于推导稳定 song id 的播放器 session
 * @param disabled host 未连接时禁止打开
 */
export function IgnoredLyricsDialog({
  document,
  session,
  disabled,
}: {
  document: LyricDocument;

  session: Session | null;

  disabled: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [mounted, setMounted] = useState(false);
  const songId = lyricSongId(document, session);

  return (
    <Dialog
      open={open}
      onOpenChange={nextOpen => {
        if (nextOpen) {
          setMounted(true);
        }

        setOpen(nextOpen);
      }}
      onOpenChangeComplete={nextOpen => {
        if (!nextOpen) {
          setMounted(false);
        }
      }}
    >
      <DialogTrigger render={<Button variant="outline" />} disabled={disabled}>
        设置忽略歌词片段
      </DialogTrigger>
      {mounted ? (
        <LyricSelection
          key={`${songId}:${document.id}`}
          document={document}
          songId={songId}
          onClose={() => setOpen(false)}
        />
      ) : null}
    </Dialog>
  );
}

function timestamp(ms: number): string {
  const seconds = Math.floor(Math.max(0, ms) / 1000);

  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}

function LyricSelection({
  document,
  songId,
  onClose,
}: {
  document: LyricDocument;

  songId: string;

  onClose: () => void;
}) {
  const [selected, setSelected] = useState(
    () =>
      new Set(
        parseOverlayConfig(readSession().snapshot?.overlay).options.ignoredLyricLines[songId] ?? [],
      ),
  );

  const [error, setError] = useState('');
  const lines = document.tracks.find(track => track.kind === 'original')?.lines ?? [];

  function save() {
    try {
      const ignoredLyricLines = {
        ...parseOverlayConfig(readSession().snapshot?.overlay).options.ignoredLyricLines,
      };

      const chosen = lines.map(lyricLineId).filter(id => selected.has(id));

      if (chosen.length) {
        ignoredLyricLines[songId] = chosen;
      } else {
        delete ignoredLyricLines[songId];
      }

      sendOverlay({ ignoredLyricLines });
      onClose();
    } catch (error) {
      setError(String(error));
    }
  }

  return (
    <DialogContent className="flex max-h-[min(85dvh,calc(100dvh-2rem))] max-w-xl flex-col overflow-hidden p-0">
      <header className="min-w-0 shrink-0 space-y-2 border-b p-5 pr-12">
        <DialogTitle>设置忽略歌词片段</DialogTitle>
        <p className="truncate text-sm">{document.metadata.title || document.filename}</p>
        <DialogDescription className="text-xs">
          勾选的歌词行会从画面移除，其他歌词继续正常显示；配置仅用于这首歌。
        </DialogDescription>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain p-3">
        {lines.length ? (
          lines.map((line, index) => {
            const id = lyricLineId(line);

            return (
              <label
                key={index}
                className="hover:bg-muted flex cursor-pointer items-start gap-3 rounded-lg px-3 py-2 text-sm"
              >
                <input
                  type="checkbox"
                  className="accent-primary mt-1 size-4 shrink-0"
                  checked={selected.has(id)}
                  onChange={event => {
                    const checked = event.target.checked;
                    setSelected(previous => {
                      const next = new Set(previous);

                      if (checked) {
                        next.add(id);
                      } else {
                        next.delete(id);
                      }

                      return next;
                    });
                  }}
                />
                <span className="text-muted-foreground mt-0.5 shrink-0 font-mono text-xs">
                  {timestamp(line.startMs)}
                </span>
                <span className="min-w-0 break-words">{line.text || '（空行）'}</span>
              </label>
            );
          })
        ) : (
          <p className="text-muted-foreground p-3 text-sm">当前没有可配置的歌词。</p>
        )}
      </div>
      <footer className="shrink-0 space-y-3 border-t p-4">
        <FieldError show={Boolean(error)}>{error}</FieldError>
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>
            清空选择
          </Button>
          <span className="text-muted-foreground text-xs" role="status">
            已选 {lines.filter(line => selected.has(lyricLineId(line))).length} 行
          </span>
          <Button size="sm" variant="outline" className="ml-auto" onClick={onClose}>
            取消
          </Button>
          <Button size="sm" onClick={save}>
            保存
          </Button>
        </div>
      </footer>
    </DialogContent>
  );
}
