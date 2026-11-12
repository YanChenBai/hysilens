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
  inline = false,
}: {
  value: string;

  onChange: (value: string) => void;

  items: ChoiceItem[];

  label: string;

  /**
   * 设置行内使用紧凑触发器，文字靠左，箭头靠右
   */
  inline?: boolean;
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
      <SelectTrigger aria-label={label} className={inline ? 'settings-select' : undefined}>
        <SelectValue className="min-w-0 flex-1 truncate text-left" />
      </SelectTrigger>
      <SelectContent className="settings-popup rounded-2xl p-1.5">
        {items.map(item => (
          <SelectItem key={item.value} value={item.value} className="rounded-lg py-1.5">
            {item.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
