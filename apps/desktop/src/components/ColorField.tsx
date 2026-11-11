import { Input } from '@hysilens/ui';

interface Props {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
}

export function ColorField({ id, label, value, onChange }: Props) {
  const color = /^#[\da-f]{6}$/i.test(value) ? value : '#000000';

  return (
    <div className="relative">
      <Input
        id={id}
        value={value}
        className="pr-12 font-mono"
        spellCheck={false}
        autoComplete="off"
        onChange={event => onChange(event.target.value)}
      />
      <div className="focus-within:ring-ring absolute top-1/2 right-2.5 size-6 -translate-y-1/2 overflow-hidden rounded-md border focus-within:ring-2 focus-within:ring-offset-2">
        <span className="pointer-events-none absolute inset-0" style={{ background: color }} />
        <input
          type="color"
          aria-label={`${label}：选择颜色`}
          title="选择颜色"
          value={color}
          className="absolute inset-0 size-full cursor-pointer opacity-0"
          onChange={event => onChange(event.target.value)}
        />
      </div>
    </div>
  );
}
