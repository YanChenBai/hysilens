import { Field, Input, Label, Switch, Textarea } from '@hysilens/ui';
import { Braces, Undo } from 'kubai-icons-react';
import { useState } from 'react';

import { defaults, fields } from '../lib/config';
import type { ConfigValues, PluginInfo, SchemaField } from '../lib/config';
import { Choice } from './Choice';
import { IconButton } from './IconButton';

interface ControlProps {
  id: string;
  name: string;
  field: SchemaField;
  value: unknown;
  onChange: (value: unknown) => void;
}

/** Declared step, else whole numbers, except fine steps for small ranges like 0–1 opacity. */
function stepFor(field: SchemaField): number {
  const declared = field['x-hysilens-step'];

  if (declared !== undefined) {
    return declared;
  }

  if (field.type === 'integer' || field.minimum === undefined || field.maximum === undefined) {
    return 1;
  }

  return field.maximum - field.minimum <= 2 ? 0.01 : 1;
}

function NumberControl({ id, field, value, onChange }: ControlProps) {
  const ranged = field.minimum !== undefined && field.maximum !== undefined;
  const step = stepFor(field);
  const current = typeof value === 'number' ? value : Number(field.default ?? 0);

  return (
    <div className="flex items-center gap-3">
      {ranged ? (
        <input
          type="range"
          aria-label={field.title}
          className="seek h-2 min-w-0 flex-1 cursor-pointer"
          min={field.minimum}
          max={field.maximum}
          step={step}
          value={current}
          onChange={event => onChange(Number(event.target.value))}
        />
      ) : null}
      <Input
        id={id}
        type="number"
        className={ranged ? 'w-24' : undefined}
        min={field.minimum}
        max={field.maximum}
        step={step}
        value={String(current)}
        onChange={event => onChange(event.target.value === '' ? '' : Number(event.target.value))}
      />
    </div>
  );
}

function ColorControl({ id, field, value, onChange }: ControlProps) {
  const text = typeof value === 'string' ? value : '';
  const swatch = /^#[\da-f]{6}$/i.test(text) ? text : '#000000';

  return (
    <div className="relative">
      <Input
        id={id}
        value={text}
        className="pr-12 font-mono"
        spellCheck={false}
        onChange={event => onChange(event.target.value)}
      />
      <span className="absolute top-1/2 right-2.5 size-6 -translate-y-1/2 overflow-hidden rounded-md border">
        <span className="pointer-events-none absolute inset-0" style={{ background: swatch }} />
        <input
          type="color"
          aria-label={`${field.title ?? id}：选择颜色`}
          value={swatch}
          className="absolute inset-0 size-full cursor-pointer opacity-0"
          onChange={event => onChange(event.target.value)}
        />
      </span>
    </div>
  );
}

function Control(props: ControlProps) {
  const { id, name, field, value, onChange } = props;

  if (field.type === 'boolean') {
    return <Switch id={id} checked={Boolean(value)} onCheckedChange={onChange} />;
  }

  if (field.enum) {
    const labels = field['x-hysilens-enum-labels'] ?? {};

    return (
      <Choice
        id={id}
        label={field.title ?? name}
        value={String(value)}
        items={field.enum.map(item => ({
          value: String(item),
          label: labels[String(item)] ?? String(item),
        }))}
        onChange={onChange}
      />
    );
  }

  if (field.type === 'number' || field.type === 'integer') {
    return <NumberControl {...props} />;
  }

  if (field['x-hysilens-control'] === 'color') {
    return <ColorControl {...props} />;
  }

  const families = field['x-hysilens-font-families'] ?? [];

  return (
    <>
      <Input
        id={id}
        value={typeof value === 'string' ? value : ''}
        maxLength={field.maxLength}
        list={families.length ? `${id}-fonts` : undefined}
        onChange={event => onChange(event.target.value)}
      />
      {families.length ? (
        <datalist id={`${id}-fonts`}>
          {families.map(family => (
            <option key={family} value={family}>
              {family}
            </option>
          ))}
        </datalist>
      ) : null}
    </>
  );
}

function JsonEditor({
  config,
  onChange,
}: {
  config: ConfigValues;
  onChange: (config: ConfigValues) => void;
}) {
  const formatted = JSON.stringify(config, null, 2);
  const [draft, setDraft] = useState(formatted);
  const [error, setError] = useState('');
  const [followed, setFollowed] = useState(formatted);

  // Adopt edits made in the form; adjusting state during render avoids an extra effect pass.
  if (formatted !== followed) {
    setFollowed(formatted);
    setDraft(formatted);
    setError('');
  }

  return (
    <details className="rounded-lg border px-3 py-2">
      <summary className="flex cursor-pointer items-center gap-2 text-sm">
        <Braces color="#fbbf24" size={18} />
        JSON
      </summary>
      <Textarea
        aria-label="配置 JSON"
        className="mt-3 min-h-48 font-mono text-xs"
        spellCheck={false}
        value={draft}
        onChange={event => {
          setDraft(event.target.value);

          try {
            onChange(JSON.parse(event.target.value) as ConfigValues);
            setError('');
          } catch (problem) {
            setError(problem instanceof Error ? problem.message : String(problem));
          }
        }}
      />
      {error ? <p className="text-destructive mt-1 text-xs">{error}</p> : null}
    </details>
  );
}

export function ConfigPanel({
  info,
  config,
  onChange,
}: {
  info: PluginInfo;
  config: ConfigValues;
  onChange: (config: ConfigValues) => void;
}) {
  const entries = fields(info);

  return (
    <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-4 pb-6">
      <div className="flex items-center justify-between">
        <p className="text-muted-foreground text-xs">修改即时生效，仅保存在本次会话中。</p>
        <IconButton label="恢复默认值" onClick={() => onChange(defaults(info))}>
          <Undo color="#94a3b8" size={18} trigger="parent" />
        </IconButton>
      </div>
      {entries.length ? null : (
        <p className="text-muted-foreground text-sm">此插件没有可配置项。</p>
      )}
      {entries.map(([name, field]) => {
        const id = `config-${name}`;

        return (
          <Field
            key={name}
            className={
              field.type === 'boolean' ? 'flex-row items-center justify-between gap-4' : undefined
            }
          >
            <Label htmlFor={id}>{field.title ?? name}</Label>
            <Control
              id={id}
              name={name}
              field={field}
              value={config[name]}
              onChange={value => onChange({ ...config, [name]: value })}
            />
          </Field>
        );
      })}
      <JsonEditor config={config} onChange={onChange} />
    </div>
  );
}
