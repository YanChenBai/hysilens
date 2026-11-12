import type { PluginManifest } from '@hysilens/plugin';
import { Button, FieldError, Textarea } from '@hysilens/ui';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import type { AnySchemaObject } from 'ajv';
import { Code, Languages, Palette, Sliders, Type } from 'kubai-icons-react';
import { Save } from 'lucide-react';
import { useState } from 'react';

import { checkConfig } from '../lib/api';
import type { PluginConfig } from '../lib/api';
import { control } from '../lib/control';
import { FooterActions } from './FooterActions';
import { SettingsSection } from './SettingsSection';
import { ThemeField } from './ThemeField';

/**
 * 面向单个 plugin 的 schema 驱动主题编辑器
 *
 * 把 JSON 草稿放在 query 缓存里，切换标签页不会丢失编辑；保存时按最新安装的
 * manifest 校验；在保存按钮旁显示 dirty 状态
 *
 * @param plugin 当前活跃的 manifest；其 `revision` 决定 config query 的范围
 * @param initial 已存储的 config 以及用于构建字段的 schema
 */
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
    /* 校验之前允许继续编辑非法 JSON */
  }

  /*
   * schema 只声明可配置项，因此每个顶层属性都映射到一个控件
   * 不支持的类型回退为原始 JSON 编辑
   */
  const fields = Object.entries(initial.schema.properties ?? {}) as Array<
    [string, AnySchemaObject]
  >;

  const commonFields = fields.filter(([name]) => ['romanization', 'translation'].includes(name));

  const sizingNames = ['fontFamily', 'fontSize', 'fontWeight', 'scale', 'activeScale'];
  const sizingFields = sizingNames.flatMap(name => fields.filter(([key]) => key === name));

  const themeFields = fields.filter(
    ([name]) => name !== 'romanization' && name !== 'translation' && !sizingNames.includes(name),
  );

  const isColor = ([, field]: [string, AnySchemaObject]) =>
    field['x-hysilens-control'] === 'color' || field.pattern === '^#[0-9a-fA-F]{6}$';

  const colorFields = themeFields.filter(isColor);
  const effectFields = themeFields.filter(field => !isColor(field));

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
      const catalog = await control.listPlugins();
      const latest = catalog.plugins.find(item => item.id === plugin.id);

      if (!latest) {
        throw new Error('插件暂不可用');
      }

      const config = checkConfig(latest, JSON.parse(submittedDraft));
      await control.setPluginConfig(plugin.id, config);

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
      className="min-w-0"
      onSubmit={event => {
        event.preventDefault();

        if (!dirty || save.isPending) {
          return;
        }

        setMessage('');
        save.mutate(draft);
      }}
    >
      <div className="space-y-7">
        {sizingFields.length ? (
          <SettingsSection title="字体与缩放" icon={<Type color="#60a5fa" size={20} />}>
            {sizingFields.map(([name, field]) => (
              <ThemeField
                key={name}
                name={name}
                field={field}
                value={values[name]}
                onChange={value => setField(name, value)}
              />
            ))}
          </SettingsSection>
        ) : null}
        {commonFields.length ? (
          <SettingsSection title="歌词显示" icon={<Languages color="#34d399" size={20} />}>
            {commonFields.map(([name, field]) => (
              <ThemeField
                key={name}
                name={name}
                field={field}
                value={values[name]}
                onChange={value => setField(name, value)}
              />
            ))}
          </SettingsSection>
        ) : null}
        {colorFields.length ? (
          <SettingsSection title="配色" icon={<Palette color="#a78bfa" size={20} />}>
            {colorFields.map(([name, field]) => (
              <ThemeField
                key={name}
                name={name}
                field={field}
                value={values[name]}
                onChange={value => setField(name, value)}
              />
            ))}
          </SettingsSection>
        ) : null}
        {effectFields.length ? (
          <SettingsSection title="版式与效果" icon={<Sliders color="#e9b44c" size={20} />}>
            {effectFields.map(([name, field]) => (
              <ThemeField
                key={name}
                name={name}
                field={field}
                value={values[name]}
                onChange={value => setField(name, value)}
              />
            ))}
          </SettingsSection>
        ) : null}
        <SettingsSection title="高级设置" icon={<Code color="#60a5fa" size={20} />}>
          <details className="py-3 text-sm">
            <summary className="text-muted-foreground cursor-pointer text-xs">
              编辑主题 JSON
            </summary>
            <Textarea
              aria-label="主题 JSON"
              className="mt-3 min-h-40 font-mono text-xs"
              value={draft}
              onChange={event => updateDraft(event.target.value)}
            />
          </details>
        </SettingsSection>
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
