import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@hysilens/ui';

/**
 * {@link Choice} 下拉框中的一个选项
 */
export interface ChoiceItem {
  value: string;

  label: string;
}

/**
 * 单选下拉框，选中的值是普通字符串
 *
 * @param label 触发器的无障碍名称，仅辅助技术可见
 */
export function Choice({
  value,
  onChange,
  items,
  label,
}: {
  value: string;

  onChange: (value: string) => void;

  items: ChoiceItem[];

  label: string;
}) {
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
      <SelectTrigger aria-label={label}>
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
