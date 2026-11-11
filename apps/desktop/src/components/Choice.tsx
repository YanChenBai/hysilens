import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@hysilens/ui';

export interface ChoiceItem {
  value: string;
  label: string;
}

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
