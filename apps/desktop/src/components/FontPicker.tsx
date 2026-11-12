import { Combobox, FieldError, Spinner } from '@hysilens/ui';
import { useQuery } from '@tanstack/react-query';
import { Check, ChevronDown } from 'lucide-react';

import { control } from '../lib/control';

interface Props {
  /**
   * 输入框 id，便于外层 `Label` 引用
   */
  id: string;

  /**
   * 当前字体族名称
   */
  value: string;

  /**
   * plugin 声明但本机可能未安装的字体；会合并到选项里并排在系统字体列表之前
   */
  extraFonts: string[];

  onChange: (value: string) => void;
}

/**
 * 可搜索的字体族选择器，数据来自 host 已安装的字体列表
 */
export function FontPicker({ id, value, extraFonts, onChange }: Props) {
  const fonts = useQuery({
    queryKey: ['system-fonts'],
    queryFn: control.listFonts,
    staleTime: Infinity,
  });

  const items = Array.from(new Set([value, ...extraFonts, ...(fonts.data ?? [])])).filter(Boolean);

  return (
    <div>
      <Combobox.Root
        items={items}
        value={value}
        onValueChange={next => {
          if (typeof next === 'string') {
            onChange(next);
          }
        }}
      >
        <div className="relative">
          <Combobox.Input
            id={id}
            aria-label="字体"
            placeholder="搜索字体"
            className="settings-input settings-input-with-action w-full py-2"
          />
          <Combobox.Trigger
            aria-label="选择字体"
            className="text-muted-foreground focus-visible:ring-ring/40 absolute inset-y-0 right-0 flex w-9 items-center justify-center rounded-xl outline-none focus-visible:ring-2"
          >
            {fonts.isPending ? <Spinner /> : <ChevronDown className="size-4" />}
          </Combobox.Trigger>
        </div>
        <Combobox.Portal>
          <Combobox.Positioner sideOffset={6} className="z-50">
            <Combobox.Popup className="settings-popup bg-card text-foreground w-[var(--anchor-width)] min-w-52 overflow-hidden rounded-2xl border p-1.5 text-sm shadow-lg">
              <Combobox.Empty className="text-muted-foreground px-3 py-3 text-xs">
                没有匹配的字体
              </Combobox.Empty>
              <Combobox.List className="max-h-64 overflow-y-auto">
                {(font: string) => (
                  <Combobox.Item
                    key={font}
                    value={font}
                    className="data-highlighted:bg-muted relative cursor-default rounded-lg py-1.5 pr-8 pl-3 outline-none"
                  >
                    {font}
                    <Combobox.ItemIndicator className="absolute top-2.5 right-2">
                      <Check className="size-3.5" />
                    </Combobox.ItemIndicator>
                  </Combobox.Item>
                )}
              </Combobox.List>
            </Combobox.Popup>
          </Combobox.Positioner>
        </Combobox.Portal>
      </Combobox.Root>
      <FieldError show={Boolean(fonts.error)}>{fonts.error?.message}</FieldError>
    </div>
  );
}
