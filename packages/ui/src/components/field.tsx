import { cn } from 'cn';
import { AnimatePresence, motion, useReducedMotion } from 'motion/react';
import type { ComponentProps } from 'react';

export type FieldProps = ComponentProps<'div'>;

/**
 * 表单字段布局；将 Label、输入和提示放在同一个字段内
 * @example
 * ```tsx
 * <Field>
 *   <Label htmlFor="name">名称</Label>
 *   <Input id="name" />
 *   <FieldDescription>公开显示的名称</FieldDescription>
 * </Field>
 * ```
 */
export function Field({ className, ...props }: FieldProps) {
  return <div data-slot="field" className={cn('flex flex-col gap-2', className)} {...props} />;
}

export type LabelProps = ComponentProps<'label'>;

/**
 * 原生表单标签；使用 `htmlFor` 关联输入控件
 * @example
 * ```tsx
 * <Label htmlFor="name">名称</Label>
 * ```
 */
export function Label({ className, htmlFor, children, ...props }: LabelProps) {
  return (
    <label
      data-slot="label"
      htmlFor={htmlFor}
      className={cn('text-sm font-medium leading-none peer-disabled:opacity-50', className)}
      {...props}
    >
      {children}
    </label>
  );
}

export type FieldDescriptionProps = ComponentProps<'p'>;

/**
 * 字段辅助说明；使用 `aria-describedby` 关联输入
 * @example
 * ```tsx
 * <FieldDescription id="name-help">公开显示</FieldDescription>
 * ```
 */
export function FieldDescription({ className, ...props }: FieldDescriptionProps) {
  return (
    <p
      data-slot="field-description"
      className={cn('text-xs leading-relaxed text-muted-foreground', className)}
      {...props}
    />
  );
}

export type FieldErrorProps = ComponentProps<'p'> & {
  /**
   * 控制错误提示的进出动画；默认根据 `children` 是否存在判断
   */
  show?: boolean;
};

/**
 * 支持进出动画的错误信息；保持组件挂载，通过 `show` 控制显隐
 * @example
 * ```tsx
 * <FieldError id="name-error" show={!!error}>
 *   {error}
 * </FieldError>
 * ```
 */
export function FieldError({
  className,
  children,
  show = Boolean(children),
  ...props
}: FieldErrorProps) {
  const reducedMotion = useReducedMotion();

  return (
    <AnimatePresence initial={false}>
      {show && (
        <motion.div
          key="error"
          initial={{ height: 0, opacity: 0, y: reducedMotion ? 0 : -4 }}
          animate={{ height: 'auto', opacity: 1, y: 0 }}
          exit={{ height: 0, opacity: 0, y: reducedMotion ? 0 : -4 }}
          transition={{ duration: reducedMotion ? 0 : 0.18, ease: 'easeOut' }}
          className="overflow-hidden"
        >
          <p
            data-slot="field-error"
            role="alert"
            className={cn('text-xs text-destructive', className)}
            {...props}
          >
            {children}
          </p>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
