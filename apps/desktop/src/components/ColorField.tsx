import { Input } from '@hysilens/ui';

interface Props {
  id: string;

  /**
   * 用在原生取色器无障碍标签中的颜色名称
   */
  label: string;

  /**
   * 输入框中的十六进制颜色文本；无法解析时色块回退为黑色
   */
  value: string;

  onChange: (value: string) => void;
}

/**
 * 十六进制颜色文本输入框，搭配原生取色器色块
 */
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
