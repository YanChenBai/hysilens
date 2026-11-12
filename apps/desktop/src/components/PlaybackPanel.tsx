import type { PlaybackSource } from '@hysilens/protocol';
import { parseOverlayConfig } from '@hysilens/protocol/config';
import { FieldError, Input } from '@hysilens/ui';
import { ActivityAlert, Music, Radio } from 'kubai-icons-react';
import { useState } from 'react';

import { send, sendOverlay, useSession } from '../lib/session';
import { Choice } from './Choice';
import { IgnoredLyricsDialog } from './IgnoredLyricsDialog';
import { KugouEnhancedSetting } from './KugouEnhancedSetting';
import { SettingsRow, SettingsSection } from './SettingsSection';

const sources = [
  { value: 'auto', label: '自动选择' },
  { value: 'wesing', label: '全民 K 歌' },
  { value: 'qqMusic', label: 'QQ 音乐' },
  { value: 'kugou', label: '酷狗音乐' },
];

/**
 * playback 同步设置：监听哪个播放器、歌词偏移、歌词候选选择以及每首歌的忽略行
 */
export function PlaybackPanel() {
  const { snapshot, connection } = useSession();
  const overlay = parseOverlayConfig(snapshot?.overlay).options;
  const [error, setError] = useState('');

  function action(callback: () => void) {
    try {
      callback();
      setError('');
    } catch (error) {
      setError(String(error));
    }
  }

  return (
    <div className="min-w-0 space-y-7">
      <SettingsSection title="播放同步" icon={<Radio size={20} color="#34d399" />}>
        <SettingsRow label="监听播放器">
          <div className="w-36 max-w-full">
            <Choice
              inline
              label="监听播放器"
              value={snapshot?.source ?? 'auto'}
              items={sources}
              onChange={value =>
                action(() => send({ kind: 'source', value: value as PlaybackSource }))
              }
            />
          </div>
        </SettingsRow>
        <SettingsRow label="歌词时间偏移" htmlFor="offset">
          <div className="flex items-center gap-3">
            <Input
              id="offset"
              type="number"
              min={-10000}
              max={10000}
              className="settings-input w-24 tabular-nums"
              aria-label="歌词时间偏移（毫秒）"
              value={overlay.offset}
              onChange={event => action(() => sendOverlay({ offset: Number(event.target.value) }))}
            />
            <span className="text-muted-foreground text-xs" aria-hidden="true">
              ms
            </span>
          </div>
        </SettingsRow>
        <KugouEnhancedSetting />
      </SettingsSection>
      {snapshot?.candidates.length || snapshot?.document ? (
        <SettingsSection title="当前歌曲" icon={<Music size={20} color="#a78bfa" />}>
          {snapshot.candidates.length ? (
            <SettingsRow label="匹配歌词">
              <div className="w-44 max-w-full">
                <Choice
                  inline
                  label="匹配歌词"
                  value="auto"
                  items={[
                    { value: 'auto', label: '自动匹配' },
                    ...snapshot.candidates.map(item => ({
                      value: item.key,
                      label: item.filename,
                    })),
                  ]}
                  onChange={value =>
                    action(() =>
                      send({
                        kind: 'select',
                        generation: snapshot.generation,
                        key: value === 'auto' ? null : value,
                      }),
                    )
                  }
                />
              </div>
            </SettingsRow>
          ) : null}
          {snapshot.document ? (
            <SettingsRow label="忽略歌词片段" description="选择当前歌曲中不显示的片段">
              <IgnoredLyricsDialog
                key={snapshot.document.id}
                document={snapshot.document}
                session={snapshot.session}
                disabled={connection !== 'connected'}
              />
            </SettingsRow>
          ) : null}
        </SettingsSection>
      ) : null}
      {snapshot?.cacheIssues.length ? (
        <SettingsSection title="缓存提示" icon={<ActivityAlert size={20} color="#e9b44c" />}>
          <details className="group py-2">
            <summary className="text-muted-foreground marker:text-muted-foreground cursor-pointer text-xs leading-relaxed">
              {snapshot.cacheIssues.length} 个歌词缓存文件无法读取，查看详情
            </summary>
            <div className="mt-3 space-y-2">
              {snapshot.cacheIssues.map(issue => (
                <p
                  key={issue}
                  className="text-muted-foreground text-xs leading-relaxed wrap-anywhere"
                >
                  {issue}
                </p>
              ))}
            </div>
          </details>
        </SettingsSection>
      ) : null}
      <FieldError show={Boolean(error)}>{error}</FieldError>
    </div>
  );
}
