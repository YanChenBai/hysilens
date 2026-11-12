import { cn } from 'cn';
import type { ComponentProps } from 'react';

const inputStyles =
  'w-full rounded-xl border bg-card px-3.5 py-2.5 text-sm shadow-xs transition-colors outline-none placeholder:text-muted-foreground/70 focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/20 disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-destructive/20';

export type InputProps = ComponentProps<'input'>;

/**
 * 单行输入，透传原生属性、事件和 ref
 * @example
 * ```tsx
 * <Input id="name" aria-invalid={!!error} aria-describedby="name-error" />
 * ```
 */
export function Input({ className, type = 'text', ...props }: InputProps) {
  return (
    <input
      data-slot="input"
      type={type}
      className={cn(inputStyles, 'h-10', className)}
      {...props}
    />
  );
}

export type TextareaProps = ComponentProps<'textarea'>;

/**
 * 多行输入，透传原生属性
 * @example
 * ```tsx
 * <Textarea aria-label="备注" rows={4} />
 * ```
 */
export function Textarea({ className, ...props }: TextareaProps) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(inputStyles, 'min-h-28 resize-y', className)}
      {...props}
    />
  );
}
