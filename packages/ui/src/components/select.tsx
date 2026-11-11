import { Select as BaseSelect } from '@base-ui/react/select';
import { cn } from 'cn';
import { Check, ChevronDown } from 'lucide-react';

import type { WithClassName } from '../types';

/**
 * 静态选项选择器；搜索和远程分页请使用 SearchableSelect。
 * @example
 * ```tsx
 * <Select defaultValue="all">
 *   <SelectTrigger aria-label="状态">
 *     <SelectValue />
 *   </SelectTrigger>
 *   <SelectContent>
 *     <SelectItem value="all">全部</SelectItem>
 *   </SelectContent>
 * </Select>
 * ```
 */
export const Select = BaseSelect.Root;

/**
 * 展示当前选项或占位文字
 * @example
 * ```tsx
 * <SelectValue placeholder="请选择" />
 * ```
 */
export const SelectValue = BaseSelect.Value;

/**
 * 选择器分组标题
 * @example
 * ```tsx
 * <SelectLabel>状态</SelectLabel>
 * ```
 */
export const SelectLabel = BaseSelect.Label;

/**
 * 选择器选项分组
 * @example
 * ```tsx
 * <SelectGroup>
 *   <SelectLabel>状态</SelectLabel>
 *   <SelectItem value="active">启用</SelectItem>
 * </SelectGroup>
 * ```
 */
export const SelectGroup = BaseSelect.Group;

export type SelectTriggerProps = WithClassName<BaseSelect.Trigger.Props>;

/**
 * 选择器触发按钮；无可见标签时提供 aria-label
 * @example
 * ```tsx
 * <SelectTrigger aria-label="状态">
 *   <SelectValue placeholder="请选择" />
 * </SelectTrigger>
 * ```
 */
export function SelectTrigger({ className, children, ...props }: SelectTriggerProps) {
  return (
    <BaseSelect.Trigger
      data-slot="select-trigger"
      className={cn(
        'flex h-10 w-full items-center justify-between gap-2 rounded-xl border bg-card px-3 text-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-45 data-popup-open:border-ring',
        className,
      )}
      {...props}
    >
      {children}
      <BaseSelect.Icon>
        <ChevronDown
          aria-hidden="true"
          className="size-4 transition-transform duration-200 in-data-popup-open:rotate-180"
        />
      </BaseSelect.Icon>
    </BaseSelect.Trigger>
  );
}

export type SelectContentProps = WithClassName<BaseSelect.Popup.Props> &
  Pick<BaseSelect.Positioner.Props, 'sideOffset' | 'align' | 'side'>;

/**
 * 选择器选项弹层
 * @example
 * ```tsx
 * <SelectContent>
 *   <SelectItem value="all">全部</SelectItem>
 * </SelectContent>
 * ```
 */
export function SelectContent({
  className,
  children,
  sideOffset = 6,
  align = 'start',
  side,
  ...props
}: SelectContentProps) {
  return (
    <BaseSelect.Portal>
      <BaseSelect.Positioner
        side={side}
        sideOffset={sideOffset}
        align={align}
        alignItemWithTrigger={false}
        className="z-50 outline-none"
      >
        <BaseSelect.Popup
          data-slot="select-content"
          className={cn(
            'ui-popup min-w-[var(--anchor-width)] overflow-hidden rounded-2xl border bg-card p-1.5 text-sm text-foreground shadow-xl outline-none',
            className,
          )}
          {...props}
        >
          <BaseSelect.List className="max-h-[min(20rem,var(--available-height))] overflow-y-auto">
            {children}
          </BaseSelect.List>
        </BaseSelect.Popup>
      </BaseSelect.Positioner>
    </BaseSelect.Portal>
  );
}

export type SelectItemProps = WithClassName<BaseSelect.Item.Props>;

/**
 * 可选项，value 应保持唯一
 * @example
 * ```tsx
 * <SelectItem value="all">全部</SelectItem>
 * ```
 */
export function SelectItem({ className, children, ...props }: SelectItemProps) {
  return (
    <BaseSelect.Item
      className={cn(
        'relative flex cursor-default items-center rounded-lg py-2 pr-9 pl-3 outline-none select-none data-highlighted:bg-muted data-disabled:pointer-events-none data-disabled:opacity-45',
        className,
      )}
      {...props}
    >
      <BaseSelect.ItemText>{children}</BaseSelect.ItemText>
      <BaseSelect.ItemIndicator className="absolute right-3">
        <Check aria-hidden="true" className="size-4" />
      </BaseSelect.ItemIndicator>
    </BaseSelect.Item>
  );
}
