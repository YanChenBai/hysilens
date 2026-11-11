import { Button, Tooltip, TooltipContent, TooltipTrigger } from '@hysilens/ui';
import type { ButtonProps } from '@hysilens/ui';
import { cn } from 'cn';

/** Icon-only ghost button; `label` is both the tooltip and the accessible name. */
export function IconButton({
  label,
  pressed,
  className,
  children,
  ...props
}: Omit<ButtonProps, 'aria-label' | 'title' | 'variant' | 'size'> & {
  label: string;
  /** Renders as a toggle and highlights while on. */
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
