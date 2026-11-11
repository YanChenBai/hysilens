import type { LyricDocument, Session } from '@hysilens/protocol';
import { parseOverlayConfig } from '@hysilens/protocol/config';
import { Button, FieldError } from '@hysilens/ui';
import { useEffect, useRef, useState } from 'react';

import { lyricLineId, lyricSongId } from '../lib/ignored-lyrics';
import { readSession, sendOverlay } from '../lib/session';

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
  const songId = lyricSongId(document, session);

  return (
    <>
      <Button variant="outline" disabled={disabled} onClick={() => setOpen(true)}>
        设置忽略歌词片段
      </Button>
      {open ? (
        <LyricSelection
          key={`${songId}:${document.id}`}
          document={document}
          songId={songId}
          onClose={() => setOpen(false)}
        />
      ) : null}
    </>
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
  const dialog = useRef<HTMLDialogElement>(null);

  const [selected, setSelected] = useState(
    () =>
      new Set(
        parseOverlayConfig(readSession().snapshot?.overlay).options.ignoredLyricLines[songId] ?? [],
      ),
  );

  const [error, setError] = useState('');
  const lines = document.tracks.find(track => track.kind === 'original')?.lines ?? [];

  useEffect(() => {
    const element = dialog.current;
    element?.showModal();

    return () => element?.close();
  }, []);

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
    <dialog
      ref={dialog}
      aria-labelledby="ignored-lyrics-title"
      aria-describedby="ignored-lyrics-description"
      onCancel={onClose}
      className="bg-background text-foreground fixed inset-0 m-auto max-h-[85dvh] w-[min(36rem,calc(100vw-2rem))] rounded-2xl border p-0 shadow-xl backdrop:bg-black/50"
    >
      <div className="flex max-h-[85dvh] flex-col">
        <header className="shrink-0 space-y-2 border-b p-5">
          <h2 id="ignored-lyrics-title" className="font-semibold">
            设置忽略歌词片段
          </h2>
          <p className="truncate text-sm">{document.metadata.title || document.filename}</p>
          <p id="ignored-lyrics-description" className="text-muted-foreground text-xs">
            勾选的歌词行会从画面移除，其他歌词继续正常显示；配置仅用于这首歌。
          </p>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto p-3">
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
      </div>
    </dialog>
  );
}
