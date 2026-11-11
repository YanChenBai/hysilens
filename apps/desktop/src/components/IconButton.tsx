import { Button, Tooltip, TooltipContent, TooltipTrigger } from '@hysilens/ui';
import type { ButtonProps } from '@hysilens/ui';

/** Icon-only ghost button whose label is both its accessible name and a hover tooltip. */
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
