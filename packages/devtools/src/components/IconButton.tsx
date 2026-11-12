import { Button, Tooltip, TooltipContent, TooltipTrigger } from '@hysilens/ui';
import type { ButtonProps } from '@hysilens/ui';
import { cn } from 'cn';

/**
 * 纯图标 ghost 按钮；`label` 同时作为 tooltip 和无障碍名称
 */
export function IconButton({
  label,
  pressed,
  className,
  children,
  ...props
}: Omit<ButtonProps, 'aria-label' | 'title' | 'variant' | 'size'> & {
  label: string;
  /**
   * 作为 toggle 渲染，开启时高亮
   */
  pressed?: boolean;
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            {...props}
            variant="ghost"
            size="icon"
            aria-label={label}
            aria-pressed={pressed}
            className={cn('size-9', pressed && 'bg-muted', className)}
          />
        }
      >
        {children}
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
