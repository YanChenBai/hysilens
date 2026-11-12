import type { AppSettings } from '@hysilens/protocol';
import { Button, Field, FieldError, Input, Label } from '@hysilens/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Folder, Settings } from 'kubai-icons-react';
import { Check } from 'lucide-react';
import { useState } from 'react';

import { getJson, putJson } from '../lib/api';
import { getDesktopBridge } from '../lib/local-overlay';
import { DirectoryInput } from './DirectoryInput';
import { FooterActions } from './FooterActions';
import { LocalOverlayControls } from './LocalOverlayControls';
import { PlaybackDiagnostics } from './PlaybackDiagnostics';
import { SettingsRow, SettingsSection } from './SettingsSection';

/**
 * 加载一次系统设置并交给可编辑表单
 */
export function SettingsPanel() {
  const settings = useQuery({
    queryKey: ['settings'],
    queryFn: () => getJson<AppSettings>('/api/settings'),
  });

  if (settings.error) {
    return <p className="text-destructive">{settings.error.message}</p>;
  }

  return settings.data ? (
    <SettingsForm key={settings.data.port} initial={settings.data} />
  ) : (
    <p>正在读取设置…</p>
  );
}

/**
 * 设置的可编辑副本；保存时把三个用户可编辑字段合并进最新的服务端状态
 * 这样别处并发做出的修改不会被覆盖
 */
function SettingsForm({ initial }: { initial: AppSettings }) {
  const [draft, setDraft] = useState(initial);
  const [saved, setSaved] = useState(initial);
  const [message, setMessage] = useState('');
  const queries = useQueryClient();

  const dirty =
    draft.port !== saved.port ||
    draft.cacheDir !== saved.cacheDir ||
    draft.kugouCacheDir !== saved.kugouCacheDir;

  const save = useMutation({
    mutationFn: async () => {
      const latest = await getJson<AppSettings>('/api/settings');

      const next = {
        ...latest,
        port: draft.port,
        cacheDir: draft.cacheDir,
        kugouCacheDir: draft.kugouCacheDir,
      };

      await putJson('/api/settings', next);

      return next;
    },
    onSuccess: next => {
      setSaved(next);
      setMessage('已保存，正在重新连接服务。');
      queries.setQueryData(['settings'], next);

      if (!getDesktopBridge() && next.port !== initial.port) {
        window.setTimeout(() => {
          location.href = `http://127.0.0.1:${next.port}/control`;
        }, 1000);
      }
    },
  });

  return (
    <div className="flex min-w-0 flex-col">
      <div className="space-y-7">
        <LocalOverlayControls />
        <form
          id="system-settings"
          className="space-y-7"
          onSubmit={event => {
            event.preventDefault();

            if (!dirty || save.isPending) {
              return;
            }

            save.mutate();
          }}
        >
          <SettingsSection title="歌词缓存" icon={<Folder size={20} color="#e9b44c" />}>
            <Field className="py-2">
              <Label htmlFor="qq-cache">QQ 音乐</Label>
              <DirectoryInput
                id="qq-cache"
                label="QQ 音乐歌词缓存目录"
                value={draft.cacheDir}
                onChange={value => setDraft(current => ({ ...current, cacheDir: value }))}
              />
            </Field>
            <Field className="py-2">
              <Label htmlFor="kugou-cache">酷狗音乐</Label>
              <DirectoryInput
                id="kugou-cache"
                label="酷狗歌词缓存目录"
                value={draft.kugouCacheDir}
                onChange={value => setDraft(current => ({ ...current, kugouCacheDir: value }))}
              />
            </Field>
          </SettingsSection>
          <SettingsSection title="本机服务" icon={<Settings size={20} color="#60a5fa" />}>
            <SettingsRow
              label="本机端口"
              htmlFor="port"
              description="更改后需更新 OBS 浏览器源地址"
            >
              <Input
                id="port"
                type="number"
                min={1}
                max={65535}
                className="settings-input w-24 tabular-nums"
                value={draft.port}
                onChange={event => setDraft({ ...draft, port: Number(event.target.value) })}
              />
            </SettingsRow>
          </SettingsSection>
        </form>
        <PlaybackDiagnostics />
      </div>
      <FooterActions>
        <FieldError show={Boolean(save.error)}>{save.error?.message}</FieldError>
        <p
          role="status"
          title={message || undefined}
          className={
            dirty
              ? 'text-[11px] text-amber-600 dark:text-amber-400'
              : 'text-muted-foreground text-[11px]'
          }
        >
          {dirty ? '未保存' : '已保存'}
        </p>
        <Button
          form="system-settings"
          type="submit"
          size="sm"
          disabled={!dirty}
          isLoading={save.isPending}
          title="保存并重启服务"
        >
          <Check className="size-3.5" />
          保存设置
        </Button>
      </FooterActions>
    </div>
  );
}
