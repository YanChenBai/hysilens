import { cn } from 'cn';
import { LoaderCircle } from 'lucide-react';
import type { ComponentProps, ReactNode } from 'react';

export type SpinnerProps = ComponentProps<typeof LoaderCircle>;

/**
 * 装饰性的加载图标；独立使用时为外层提供可访问的加载状态。
 * @example
 * ```tsx
 * <button aria-busy={loading}>
 *   <Spinner />
 *   保存中
 * </button>
 * ```
 */
export function Spinner({ className, ...props }: SpinnerProps) {
  return (
    <LoaderCircle
      aria-hidden="true"
      className={cn('size-4 shrink-0 animate-spin motion-reduce:animate-none', className)}
      {...props}
    />
  );
}

export type LoadingProps = Omit<ComponentProps<'span'>, 'children'> & {
  /** 可访问的标签文字。 */
  label?: ReactNode;
};

/**
 * 包含可访问状态文字的加载提示。
 * @example
 * ```tsx
 * <Loading label="正在加载成员…" />
 * ```
 */
export function Loading({ label = '加载中…', className, ...props }: LoadingProps) {
  return (
    <span
      data-slot="loading"
      role="status"
      aria-live="polite"
      className={cn('inline-flex items-center gap-2 text-sm text-muted-foreground', className)}
      {...props}
    >
      <Spinner />
      {label}
    </span>
  );
}
