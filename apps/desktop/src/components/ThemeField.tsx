import { Field, Input, Label, Switch } from '@hysilens/ui';
import type { AnySchemaObject } from 'ajv';

import { Choice } from './Choice';
import { ColorField } from './ColorField';
import { FontPicker } from './FontPicker';

interface Props {
  name: string;
  field: AnySchemaObject;
  value: unknown;
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
    return (
      <Input
        id={id}
        type="number"
        min={field.minimum}
        max={field.maximum}
        step={field.multipleOf ?? (field.type === 'integer' ? 1 : 'any')}
        value={String(current)}
        onChange={event => onChange(event.target.value === '' ? '' : Number(event.target.value))}
      />
    );
  }

  if (field.type === 'string') {
    return <StringControl name={name} field={field} value={current} onChange={onChange} />;
  }

  return <p className="text-muted-foreground text-xs">请在主题 JSON 中编辑此项。</p>;
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

  return <Input id={id} value={String(value)} onChange={event => onChange(event.target.value)} />;
}

export function ThemeField(props: Props) {
  return (
    <Field
      className={
        props.field.type === 'boolean'
          ? 'flex-row items-center justify-between gap-4 py-2'
          : undefined
      }
    >
      <Label htmlFor={`theme-${props.name}`}>{props.field.title ?? props.name}</Label>
      <Control {...props} />
    </Field>
  );
}
