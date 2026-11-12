import { Button, Tooltip, TooltipContent, TooltipTrigger } from '@hysilens/ui';
import type { ButtonProps } from '@hysilens/ui';

/**
 * 纯图标 ghost 按钮，label 同时作为无障碍名称与悬停 tooltip
 */
export function IconButton({
  label,
  children,
  ...props
}: Omit<ButtonProps, 'aria-label' | 'title' | 'variant' | 'size'> & { label: string }) {
  return (
    <Tooltip>
      <TooltipTrigger render={<Button {...props} variant="ghost" size="icon" aria-label={label} />}>
        {children}
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
