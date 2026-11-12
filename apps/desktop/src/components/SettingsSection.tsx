import { Label } from '@hysilens/ui';
import { useId } from 'react';
import type { ReactNode } from 'react';

/**
 * 轻量设置分区，标题位于浅色底板外，同组设置行共用背景
 */
export function SettingsSection({
  title,
  description,
  icon,
  children,
}: {
  title: string;

  description?: string;

  icon?: ReactNode;

  children: ReactNode;
}) {
  const heading = useId();

  return (
    <section aria-labelledby={heading} className="min-w-0 space-y-2">
      <div className="space-y-1">
        <h2
          id={heading}
          className="text-muted-foreground flex items-center gap-2 text-sm font-semibold"
        >
          {icon ? (
            <span aria-hidden="true" className="flex size-5 shrink-0 items-center justify-center">
              {icon}
            </span>
          ) : null}
          {title}
        </h2>
        {description ? (
          <p className="text-muted-foreground text-xs leading-relaxed">{description}</p>
        ) : null}
      </div>
      <div className="bg-muted/35 space-y-1 rounded-2xl px-3 py-1 [&_[data-slot=label]]:font-normal">
        {children}
      </div>
    </section>
  );
}

/**
 * 短选项的横向布局；说明跟随名称，右侧控件保持紧凑并限制宽度
 */
export function SettingsRow({
  label,
  description,
  descriptionId,
  htmlFor,
  children,
}: {
  label: string;

  description?: string;

  descriptionId?: string;

  htmlFor?: string;

  children: ReactNode;
}) {
  return (
    <div className="flex min-h-12 min-w-0 items-center justify-between gap-4 py-2">
      <div className="min-w-0 flex-1 space-y-1">
        {htmlFor ? (
          <Label htmlFor={htmlFor} className="text-sm leading-5">
            {label}
          </Label>
        ) : (
          <p className="text-sm leading-5">{label}</p>
        )}
        {description ? (
          <p id={descriptionId} className="text-muted-foreground text-xs leading-relaxed">
            {description}
          </p>
        ) : null}
      </div>
      <div className="flex max-w-[52%] min-w-0 shrink-0 items-center justify-end">{children}</div>
    </div>
  );
}
