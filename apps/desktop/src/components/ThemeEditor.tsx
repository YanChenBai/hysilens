import type { PluginManifest } from '@hysilens/plugin';
import { Button, FieldError, Textarea } from '@hysilens/ui';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import type { AnySchemaObject } from 'ajv';
import { Languages, Palette } from 'kubai-icons-react';
import { Save } from 'lucide-react';
import { useState } from 'react';

import { checkConfig, getJson, putJson } from '../lib/api';
import type { PluginCatalog, PluginConfig } from '../lib/api';
import { FooterActions } from './FooterActions';
import { ThemeField } from './ThemeField';

export function ThemeEditor({
  plugin,
  initial,
}: {
  plugin: PluginManifest;
  initial: PluginConfig;
}) {
  const queries = useQueryClient();
  const [savedDraft, setSavedDraft] = useState(JSON.stringify(initial.config, null, 2));

  const [draft, setDraft] = useState(
    () => queries.getQueryData<string>(['theme-draft', plugin.id]) ?? savedDraft,
  );

  const [message, setMessage] = useState('');
  const dirty = draft !== savedDraft;
  let values: Record<string, unknown> = {};

  try {
    values = JSON.parse(draft);
  } catch {
    /* Keep invalid JSON editable until validation. */
  }

  // The schema already contains only declared features, so every property is editable.
  const fields = Object.entries(initial.schema.properties ?? {}) as Array<
    [string, AnySchemaObject]
  >;

  const commonFields = fields.filter(([name]) => ['romanization', 'translation'].includes(name));

  const sizingNames = ['fontFamily', 'fontSize', 'fontWeight', 'scale', 'activeScale'];
  const sizingFields = sizingNames.flatMap(name => fields.filter(([key]) => key === name));

  const themeFields = fields.filter(
    ([name]) => name !== 'romanization' && name !== 'translation' && !sizingNames.includes(name),
  );

  const themeControls = themeFields.filter(([, field]) => field.type !== 'boolean');
  const themeSwitches = themeFields.filter(([, field]) => field.type === 'boolean');

  function setField(name: string, value: unknown) {
    updateDraft(JSON.stringify({ ...values, [name]: value }, null, 2));
  }

  function updateDraft(value: string) {
    setDraft(value);
    queries.setQueryData(['theme-draft', plugin.id], value);
    setMessage('');
  }

  const save = useMutation({
    mutationFn: async (submittedDraft: string) => {
      const catalog = await getJson<PluginCatalog>('/api/plugins');
      const latest = catalog.plugins.find(item => item.id === plugin.id);

      if (!latest) {
        throw new Error('插件暂不可用');
      }

      const config = checkConfig(latest, JSON.parse(submittedDraft));
      await putJson(`/api/plugins/${plugin.id}/config`, config);

      return config;
    },
    onSuccess: (config, submittedDraft) => {
      const saved = JSON.stringify(config, null, 2);
      setSavedDraft(saved);
      setDraft(current => (current === submittedDraft ? saved : current));
      queries.setQueryData<string>(['theme-draft', plugin.id], current =>
        current === submittedDraft ? saved : current,
      );
      queries.setQueryData<PluginConfig>(['config-editor', plugin.id, plugin.revision], current =>
        current ? { ...current, config } : current,
      );
      setMessage(`已保存 ${plugin.id}.json`);
      void queries.invalidateQueries({ queryKey: ['plugin-config', plugin.id] });
      void queries.invalidateQueries({ queryKey: ['plugins'] });
    },
  });

  return (
    <form
      id={`plugin-theme-${plugin.id}`}
      className="flex min-h-0 flex-1 flex-col"
      onSubmit={event => {
        event.preventDefault();

        if (!dirty || save.isPending) {
          return;
        }

        setMessage('');
        save.mutate(draft);
      }}
    >
      <div className="min-h-0 flex-1 space-y-6 overflow-y-auto px-5 pb-6">
        {sizingFields.length ? (
          <section className="space-y-4 border-t pt-5" aria-labelledby="plugin-sizing-heading">
            <h3 id="plugin-sizing-heading" className="text-sm font-medium">
              字号与缩放
            </h3>
            <div className="grid gap-5 sm:grid-cols-2">
              {sizingFields.map(([name, field]) => (
                <ThemeField
                  key={name}
                  name={name}
                  field={field}
                  value={values[name]}
                  onChange={value => setField(name, value)}
                />
              ))}
            </div>
          </section>
        ) : null}
        {commonFields.length ? (
          <section className="space-y-4 border-t pt-5" aria-labelledby="common-display-heading">
            <h3 id="common-display-heading" className="flex items-center gap-2 text-sm font-medium">
              <Languages color="#34d399" size={20} />
              通用显示
            </h3>
            <div className="grid gap-4">
              {commonFields.map(([name, field]) => (
                <ThemeField
                  key={name}
                  name={name}
                  field={field}
                  value={values[name]}
                  onChange={value => setField(name, value)}
                />
              ))}
            </div>
          </section>
        ) : null}
        {themeFields.length ? (
          <section className="space-y-4 border-t pt-5" aria-labelledby="plugin-theme-heading">
            <h3 id="plugin-theme-heading" className="flex items-center gap-2 text-sm font-medium">
              <Palette color="#a78bfa" size={20} />
              主题设置
            </h3>
            <div className="grid gap-5 sm:grid-cols-2">
              {themeControls.map(([name, field]) => (
                <ThemeField
                  key={name}
                  name={name}
                  field={field}
                  value={values[name]}
                  onChange={value => setField(name, value)}
                />
              ))}
            </div>
            {themeSwitches.map(([name, field]) => (
              <ThemeField
                key={name}
                name={name}
                field={field}
                value={values[name]}
                onChange={value => setField(name, value)}
              />
            ))}
          </section>
        ) : null}
        <details className="border-t py-3 text-sm">
          <summary className="text-muted-foreground cursor-pointer">编辑主题 JSON</summary>
          <Textarea
            aria-label="主题 JSON"
            className="mt-3 min-h-40 font-mono text-xs"
            value={draft}
            onChange={event => updateDraft(event.target.value)}
          />
        </details>
      </div>
      <FooterActions>
        <FieldError show={Boolean(save.error)}>{save.error?.message}</FieldError>
        <span
          className={
            dirty
              ? 'text-[11px] text-amber-600 dark:text-amber-400'
              : 'text-muted-foreground text-[11px]'
          }
          role="status"
          title={message || undefined}
        >
          {dirty ? '未保存' : '已保存'}
        </span>
        <Button
          form={`plugin-theme-${plugin.id}`}
          type="submit"
          size="sm"
          disabled={!dirty}
          isLoading={save.isPending}
        >
          <Save className="size-3.5" />
          保存
        </Button>
      </FooterActions>
    </form>
  );
}
