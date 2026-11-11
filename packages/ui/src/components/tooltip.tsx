import { Tooltip as BaseTooltip } from '@base-ui/react/tooltip';
import { cn } from 'cn';

import type { WithClassName } from '../types';

/**
 * 共享多个 Tooltip 的延迟设置
 * @example
 * ```tsx
 * <TooltipProvider delay={300}>...</TooltipProvider>
 * ```
 */
export const TooltipProvider = BaseTooltip.Provider;

/**
 * 提示状态容器；应放在 TooltipProvider 内。
 * @example
 * ```tsx
 * <TooltipProvider>
 *   <Tooltip>
 *     <TooltipTrigger>帮助</TooltipTrigger>
 *     <TooltipContent>查看说明</TooltipContent>
 *   </Tooltip>
 * </TooltipProvider>
 * ```
 */
export const Tooltip = BaseTooltip.Root;

/**
 * 可聚焦的提示触发器
 * @example
 * ```tsx
 * <TooltipTrigger>帮助</TooltipTrigger>
 * ```
 */
export const TooltipTrigger = BaseTooltip.Trigger;

export type TooltipContentProps = WithClassName<BaseTooltip.Popup.Props> & {
  /** 弹层相对触发器的方向。 */
  side?: BaseTooltip.Positioner.Props['side'];
};

/**
 * 定位到触发器的提示内容
 * @example
 * ```tsx
 * <TooltipContent side="top">查看说明</TooltipContent>
 * ```
 */
export function TooltipContent({ className, side = 'top', ...props }: TooltipContentProps) {
  return (
    <BaseTooltip.Portal>
      <BaseTooltip.Positioner side={side} sideOffset={8} className="z-60">
        <BaseTooltip.Popup
          data-slot="tooltip-content"
          className={cn(
            'max-w-64 origin-(--transform-origin) rounded-lg bg-primary px-3 py-1.5 text-xs text-primary-foreground shadow-md transition-[opacity,transform] duration-150 data-starting-style:scale-95 data-starting-style:opacity-0 data-ending-style:scale-95 data-ending-style:opacity-0',
            className,
          )}
          {...props}
        />
      </BaseTooltip.Positioner>
    </BaseTooltip.Portal>
  );
}
