import type { PlaybackSource } from '@hysilens/protocol';
import { parseOverlayConfig } from '@hysilens/protocol/config';
import { Field, FieldError, Input, Label } from '@hysilens/ui';
import { useState } from 'react';

import { send, sendOverlay, useSession } from '../lib/session';
import { Choice } from './Choice';

const sources = [
  { value: 'auto', label: '自动选择' },
  { value: 'wesing', label: '全民 K 歌' },
  { value: 'qqMusic', label: 'QQ 音乐' },
  { value: 'kugou', label: '酷狗音乐' },
];

export function PlaybackPanel() {
  const { snapshot } = useSession();
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
    <>
      <section className="space-y-6">
        <div>
          <h2 className="font-semibold">播放同步</h2>
        </div>
        <Field>
          <Label>监听播放器</Label>
          <Choice
            label="监听播放器"
            value={snapshot?.source ?? 'auto'}
            items={sources}
            onChange={value =>
              action(() => send({ kind: 'source', value: value as PlaybackSource }))
            }
          />
        </Field>
        <Field>
          <Label htmlFor="offset">歌词时间偏移 · ms</Label>
          <Input
            id="offset"
            type="number"
            min={-10000}
            max={10000}
            value={overlay.offset}
            onChange={event => action(() => sendOverlay({ offset: Number(event.target.value) }))}
          />
        </Field>

        {snapshot?.candidates.length ? (
          <Field>
            <Label>选择匹配歌词</Label>
            <Choice
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
          </Field>
        ) : null}
        {snapshot?.cacheIssues.map(issue => (
          <p key={issue} className="text-muted-foreground text-xs">
            {issue}
          </p>
        ))}
      </section>
      <FieldError show={Boolean(error)}>{error}</FieldError>
    </>
  );
}
