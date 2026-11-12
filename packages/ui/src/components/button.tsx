import { Button as BaseButton } from '@base-ui/react/button';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from 'cn';
import { motion, useReducedMotion } from 'motion/react';
import type { ComponentProps, ReactNode } from 'react';

import { Spinner } from './loading';

const AnimatedButton = motion.create(BaseButton);

export const buttonVariants = cva(
  'inline-flex shrink-0 items-center justify-center whitespace-nowrap font-medium transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:pointer-events-none disabled:opacity-45 [&_svg]:shrink-0',
  {
    variants: {
      variant: {
        default: 'bg-primary text-primary-foreground shadow-sm hover:bg-primary/90',
        secondary: 'bg-muted text-foreground hover:bg-muted/70',
        outline: 'border bg-card text-foreground hover:bg-muted',
        ghost: 'text-foreground hover:bg-muted',
        destructive: 'bg-destructive text-white hover:bg-destructive/90',
      },
      size: {
        sm: 'h-8 gap-1.5 rounded-lg px-3 text-xs',
        default: 'h-10 gap-2 rounded-xl px-4 text-sm',
        lg: 'h-12 gap-2.5 rounded-2xl px-6 text-sm',
        icon: 'size-10 rounded-xl',
      },
    },
    defaultVariants: { variant: 'default', size: 'default' },
  },
);

export type ButtonProps = Omit<ComponentProps<typeof AnimatedButton>, 'className' | 'children'> &
  VariantProps<typeof buttonVariants> & {
    /**
     * 按钮内容
     */
    children?: ReactNode;

    /**
     * 追加到组件根节点的样式类
     */
    className?: string;

    /**
     * 显示加载状态并禁用按钮；默认 false
     */
    isLoading?: boolean;
  };

/**
 * 带悬停和按压反馈的按钮；`render` 可组合 Base UI 触发器
 * @example
 * ```tsx
 * <Button variant="outline" onClick={save}>
 *   保存
 * </Button>
 * ```
 */
export function Button({
  className,
  variant,
  size,
  isLoading = false,
  disabled,
  children,
  ...props
}: ButtonProps) {
  const reducedMotion = useReducedMotion();
  const noMotion = reducedMotion || disabled || isLoading;

  return (
    <AnimatedButton
      {...props}
      data-slot="button"
      data-kubai-trigger=""
      aria-busy={isLoading || undefined}
      aria-label={isLoading ? props['aria-label'] || '正在处理' : props['aria-label']}
      disabled={disabled || isLoading}
      whileHover={noMotion ? undefined : { y: -1 }}
      whileTap={noMotion ? undefined : { scale: 0.96, y: 0 }}
      transition={{ type: 'spring', stiffness: 450, damping: 28 }}
      className={cn(buttonVariants({ variant, size }), className)}
    >
      {isLoading ? <Spinner /> : children}
    </AnimatedButton>
  );
}
