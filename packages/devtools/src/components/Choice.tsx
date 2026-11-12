import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@hysilens/ui';

/**
 * `Choice` 选择器的一个选项
 */
export interface ChoiceItem {
  value: string;

  label: string;
}

interface ChoiceProps {
  /**
   * 触发器的 DOM id，在由其他元素为其提供 label 时使用
   */
  id?: string;

  /**
   * 无障碍名称；触发器不显示可见 label
   */
  label: string;

  value: string;

  items: ChoiceItem[];

  onChange: (value: string) => void;

  className?: string;
}

/**
 * devtools 面板使用的紧凑型带 label 选择器
 */
export function Choice({ id, label, value, items, onChange, className }: ChoiceProps) {
  return (
    <Select
      value={value}
      items={items}
      onValueChange={next => {
        if (typeof next === 'string') {
          onChange(next);
        }
      }}
    >
      <SelectTrigger id={id} aria-label={label} className={className}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {items.map(item => (
          <SelectItem key={item.value} value={item.value}>
            {item.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
