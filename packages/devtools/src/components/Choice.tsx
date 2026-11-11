import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@hysilens/ui';

export interface ChoiceItem {
  value: string;
  label: string;
}

export function Choice({
  id,
  label,
  value,
  items,
  onChange,
  className,
}: {
  id?: string;
  label: string;
  value: string;
  items: ChoiceItem[];
  onChange: (value: string) => void;
  className?: string;
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
