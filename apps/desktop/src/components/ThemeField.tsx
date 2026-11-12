import { Field, FieldDescription, Input, Label, Switch } from '@hysilens/ui';
import type { AnySchemaObject } from 'ajv';

import { Choice } from './Choice';
import { ColorField } from './ColorField';
import { FontPicker } from './FontPicker';
import { SettingsRow } from './SettingsSection';

interface Props {
  /**
   * schema 属性名，同时也是控件 id 的后缀
   */
  name: string;

  /**
   * 描述控件及其取值范围的 JSON Schema 片段
   */
  field: AnySchemaObject;

  /**
   * 当前值；已存储的 config 早于 schema 时可能缺失
   */
  value: unknown;

  /**
   * 接收解析后的控件值；数字输入框为空时传入 `''`
   */
  onChange: (value: unknown) => void;
}

function Control({ name, field, value, onChange }: Props) {
  const id = `theme-${name}`;
  const current = value ?? field.default ?? '';

  if (field.type === 'boolean') {
    return <Switch id={id} checked={Boolean(current)} onCheckedChange={onChange} />;
  }

  if (field.enum) {
    const customLabels = field['x-hysilens-enum-labels'] as Record<string, unknown> | undefined;

    const labels: Record<string, string> = {
      vertical: '竖向',
      horizontal: '横向',
      left: '向左',
      right: '向右',
    };

    return (
      <Choice
        inline
        label={String(field.title ?? name)}
        value={String(current)}
        onChange={onChange}
        items={(field.enum as string[]).map(value => {
          const label = customLabels?.[value];

          return { value, label: typeof label === 'string' ? label : (labels[value] ?? value) };
        })}
      />
    );
  }

  if (field.type === 'number' || field.type === 'integer') {
    return <NumberControl name={name} field={field} value={current} onChange={onChange} />;
  }

  if (field.type === 'string') {
    return <StringControl name={name} field={field} value={current} onChange={onChange} />;
  }

  return <p className="text-muted-foreground text-xs">请在主题 JSON 中编辑此项。</p>;
}

function NumberControl({ name, field, value, onChange }: Props) {
  const id = `theme-${name}`;

  const step =
    field['x-hysilens-step'] ?? field.multipleOf ?? (field.type === 'integer' ? 1 : 'any');

  return (
    <Input
      id={id}
      aria-describedby={field.description ? `${id}-description` : undefined}
      type="number"
      className="settings-input w-full tabular-nums"
      min={field.minimum}
      max={field.maximum}
      step={step}
      value={String(value)}
      onChange={event => onChange(event.target.value === '' ? '' : Number(event.target.value))}
    />
  );
}

function StringControl({ name, field, value, onChange }: Props) {
  const id = `theme-${name}`;
  const isColor = field['x-hysilens-control'] === 'color' || field.pattern === '^#[0-9a-fA-F]{6}$';

  if (field['x-hysilens-control'] === 'font') {
    return (
      <FontPicker
        id={id}
        value={String(value)}
        extraFonts={field['x-hysilens-font-families'] ?? []}
        onChange={onChange}
      />
    );
  }

  if (isColor) {
    return (
      <ColorField
        id={id}
        label={String(field.title ?? name)}
        value={String(value)}
        onChange={onChange}
      />
    );
  }

  return (
    <Input
      id={id}
      className="settings-input"
      value={String(value)}
      onChange={event => onChange(event.target.value)}
    />
  );
}

function controlWidth(type: unknown) {
  if (type === 'boolean') {
    return undefined;
  }

  return ['number', 'integer'].includes(String(type)) ? 'w-24 max-w-full' : 'w-40 max-w-full';
}

/**
 * 渲染一个 plugin config 属性，控件按 JSON Schema 类型与 `x-hysilens-*` 注解选择
 */
export function ThemeField(props: Props) {
  const { field, name } = props;

  const fullWidth =
    field.type === 'string' &&
    !field.enum &&
    !['color', 'font'].includes(field['x-hysilens-control']) &&
    field.pattern !== '^#[0-9a-fA-F]{6}$';

  if (!fullWidth) {
    return (
      <SettingsRow
        label={String(field.title ?? name)}
        htmlFor={field.enum ? undefined : `theme-${name}`}
        description={field.description ? String(field.description) : undefined}
        descriptionId={`theme-${name}-description`}
      >
        <div className={controlWidth(field.type)}>
          <Control {...props} />
        </div>
      </SettingsRow>
    );
  }

  return (
    <Field className="py-2">
      <Label htmlFor={`theme-${props.name}`}>{props.field.title ?? props.name}</Label>
      <Control {...props} />
      {props.field.description ? (
        <FieldDescription id={`theme-${props.name}-description`}>
          {String(props.field.description)}
        </FieldDescription>
      ) : null}
    </Field>
  );
}
