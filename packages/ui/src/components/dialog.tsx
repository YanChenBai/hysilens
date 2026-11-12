import { Dialog as BaseDialog } from '@base-ui/react/dialog';
import { cn } from 'cn';
import { X } from 'lucide-react';
import type { ComponentProps, CSSProperties } from 'react';
import { createContext, useContext, useState } from 'react';

import type { WithClassName } from '../types';
import { Button } from './button';

const DialogOriginContext = createContext<{ x: number; y: number } | null>(null);

export type DialogProps<Payload> = BaseDialog.Root.Props<Payload> & {
  /**
   * 不经 `DialogTrigger`、由 `open` 受控打开时的展开原点（视口坐标）；未提供时按触发器或点击位置
   */
  origin?: { x: number; y: number } | null;
};

/**
 * 模态框状态容器；鼠标打开时从点击位置展开，键盘打开时从触发器展开
 * @example
 * ```tsx
 * <Dialog>
 *   <DialogTrigger>编辑</DialogTrigger>
 *   <DialogContent>
 *     <DialogTitle>编辑资料</DialogTitle>
 *     <DialogDescription>修改公开信息</DialogDescription>
 *     <DialogClose>关闭</DialogClose>
 *   </DialogContent>
 * </Dialog>
 * ```
 */
export function Dialog<Payload>({
  onOpenChange,
  origin: controlledOrigin,
  ...props
}: DialogProps<Payload>) {
  const [origin, setOrigin] = useState<{ x: number; y: number } | null>(null);

  return (
    <DialogOriginContext value={controlledOrigin === undefined ? origin : controlledOrigin}>
      <BaseDialog.Root
        {...props}
        onOpenChange={(open, details) => {
          onOpenChange?.(open, details);

          if (!open || details.isCanceled) {
            return;
          }

          const event = details.event;

          if (
            details.reason === 'trigger-press' &&
            'clientX' in event &&
            'clientY' in event &&
            event.detail > 0
          ) {
            setOrigin({ x: event.clientX, y: event.clientY });

            return;
          }

          const rect = details.trigger?.getBoundingClientRect();
          setOrigin(rect ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 } : null);
        }}
      />
    </DialogOriginContext>
  );
}

/**
 * 打开 Dialog 的触发器
 * @example
 * ```tsx
 * <DialogTrigger>打开</DialogTrigger>
 * ```
 */
export const DialogTrigger = BaseDialog.Trigger;

/**
 * 关闭所在 Dialog
 * @example
 * ```tsx
 * <DialogClose>关闭</DialogClose>
 * ```
 */
export const DialogClose = BaseDialog.Close;

export type DialogContentProps = WithClassName<BaseDialog.Popup.Props>;

/**
 * 带遮罩和位置展开动画的模态内容
 *
 * 纵向排列且自身不滚动：较长的内容放进 [`DialogBody`]，标题与操作区保持常驻
 * @example
 * ```tsx
 * <DialogContent>
 *   <DialogTitle>标题</DialogTitle>
 *   <DialogBody>
 *     <DialogDescription>说明</DialogDescription>
 *   </DialogBody>
 *   <DialogFooter>…</DialogFooter>
 * </DialogContent>
 * ```
 */
export function DialogContent({ className, children, style, ...props }: DialogContentProps) {
  const origin = useContext(DialogOriginContext);

  return (
    <BaseDialog.Portal>
      <BaseDialog.Backdrop
        data-slot="dialog-backdrop"
        className="fixed inset-0 z-50 bg-black/35 backdrop-blur-sm transition-opacity duration-200 data-ending-style:opacity-0 data-starting-style:opacity-0"
      />
      <BaseDialog.Popup
        {...props}
        data-slot="dialog-content"
        style={state =>
          ({
            '--dialog-x': origin ? `calc(${origin.x}px - 50vw)` : '0px',
            '--dialog-y': origin ? `calc(${origin.y}px - 50dvh)` : '0px',
            ...(typeof style === 'function' ? style(state) : style),
          }) as CSSProperties
        }
        className={cn(
          'ui-dialog fixed top-1/2 left-1/2 z-50 max-h-[85dvh] w-[calc(100%-2rem)] max-w-md -translate-x-1/2 -translate-y-1/2 flex flex-col overflow-hidden rounded-3xl border bg-card p-6 pb-5 shadow-2xl outline-none',
          className,
        )}
      >
        {children}
        <BaseDialog.Close
          render={
            <Button
              variant="ghost"
              size="icon"
              className="absolute top-3 right-3 size-8"
              aria-label="关闭弹窗"
            />
          }
        >
          <X aria-hidden="true" className="size-4" />
        </BaseDialog.Close>
      </BaseDialog.Popup>
    </BaseDialog.Portal>
  );
}

export type DialogTitleProps = WithClassName<BaseDialog.Title.Props>;

/**
 * 模态框可访问标题
 * @example
 * ```tsx
 * <DialogTitle>编辑资料</DialogTitle>
 * ```
 */
export function DialogTitle({ className, ...props }: DialogTitleProps) {
  return (
    <BaseDialog.Title
      className={cn('shrink-0 pr-8 text-lg font-semibold tracking-tight', className)}
      {...props}
    />
  );
}

export type DialogDescriptionProps = WithClassName<BaseDialog.Description.Props>;

/**
 * 模态框说明
 * @example
 * ```tsx
 * <DialogDescription>修改公开信息</DialogDescription>
 * ```
 */
export function DialogDescription({ className, ...props }: DialogDescriptionProps) {
  return (
    <BaseDialog.Description
      className={cn('mt-2 text-sm leading-relaxed text-muted-foreground', className)}
      {...props}
    />
  );
}

export type DialogFooterProps = ComponentProps<'div'>;

/**
 * 模态框操作区
 * @example
 * ```tsx
 * <DialogFooter>
 *   <DialogClose>关闭</DialogClose>
 * </DialogFooter>
 * ```
 */
export function DialogFooter({ className, ...props }: DialogFooterProps) {
  return <div className={cn('mt-4 flex shrink-0 justify-end gap-2', className)} {...props} />;
}

export type DialogBodyProps = ComponentProps<'div'>;

/**
 * 模态框中可滚动的正文；内容超出时只有这一部分滚动，标题与操作区常驻
 * @example
 * ```tsx
 * <DialogBody>
 *   <DialogDescription>较长的说明</DialogDescription>
 * </DialogBody>
 * ```
 */
export function DialogBody({ className, ...props }: DialogBodyProps) {
  return (
    <div
      data-slot="dialog-body"
      className={cn('-mx-6 min-h-0 flex-1 overflow-y-auto overscroll-contain px-6', className)}
      {...props}
    />
  );
}
