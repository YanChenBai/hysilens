import { Switch as BaseSwitch } from '@base-ui/react/switch';
import { cn } from 'cn';

import type { WithClassName } from '../types';

export type SwitchProps = WithClassName<BaseSwitch.Root.Props>;

/**
 * 带滑块位移动画的开关。
 * @example
 * ```tsx
 * <Switch aria-label="接收通知" checked={enabled} onCheckedChange={setEnabled} />
 * ```
 */
export function Switch({ className, ...props }: SwitchProps) {
  return (
    <BaseSwitch.Root
      data-slot="switch"
      className={cn(
        'inline-flex h-6 w-10 shrink-0 items-center rounded-full border border-transparent bg-border p-0.5 outline-none transition-colors duration-250 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background data-checked:bg-primary data-disabled:cursor-not-allowed data-disabled:opacity-45',
        className,
      )}
      {...props}
    >
      <BaseSwitch.Thumb className="bg-card data-checked:bg-primary-foreground block size-4.5 translate-x-0 rounded-full shadow-xs transition-[translate,background-color] duration-250 ease-[cubic-bezier(0.34,1.56,0.64,1)] data-checked:translate-x-4" />
    </BaseSwitch.Root>
  );
}
