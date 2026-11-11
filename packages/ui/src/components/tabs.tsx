import { Tabs as BaseTabs } from '@base-ui/react/tabs';
import { cn } from 'cn';

import type { WithClassName } from '../types';

export type TabsProps = WithClassName<BaseTabs.Root.Props>;

/**
 * 支持键盘导航的标签页；TabsList 内置随选中项滑动的指示背景。
 * @example
 * ```tsx
 * <Tabs defaultValue="overview">
 *   <TabsList>
 *     <TabsTrigger value="overview">概览</TabsTrigger>
 *     <TabsTrigger value="members">成员</TabsTrigger>
 *   </TabsList>
 *   <TabsContent value="overview">概览内容</TabsContent>
 *   <TabsContent value="members">成员内容</TabsContent>
 * </Tabs>
 * ```
 */
export function Tabs({ className, ...props }: TabsProps) {
  return (
    <BaseTabs.Root
      data-slot="tabs"
      className={cn(
        'data-[orientation=vertical]:flex data-[orientation=vertical]:gap-4',
        className,
      )}
      {...props}
    />
  );
}

export type TabsListProps = WithClassName<BaseTabs.List.Props> & {
  /** 覆盖标签页选中背景的样式类。 */
  indicatorClassName?: string;
};

/**
 * 标签触发器容器，内置滑动指示背景
 * @example
 * ```tsx
 * <TabsList>
 *   <TabsTrigger value="overview">概览</TabsTrigger>
 * </TabsList>
 * ```
 */
export function TabsList({ className, indicatorClassName, children, ...props }: TabsListProps) {
  return (
    <BaseTabs.List
      data-slot="tabs-list"
      activateOnFocus
      className={cn(
        'relative isolate inline-flex max-w-full items-center gap-1 overflow-x-auto rounded-xl bg-muted p-1 data-[orientation=vertical]:flex-col data-[orientation=vertical]:items-stretch',
        className,
      )}
      {...props}
    >
      {children}
      <BaseTabs.Indicator
        data-slot="tabs-indicator"
        className={cn(
          'pointer-events-none absolute top-0 left-0 z-0 h-[var(--active-tab-height)] w-[var(--active-tab-width)] translate-x-[var(--active-tab-left)] translate-y-[var(--active-tab-top)] rounded-lg bg-card shadow-xs transition-[translate,width,height] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]',
          indicatorClassName,
        )}
      />
    </BaseTabs.List>
  );
}

export type TabsTriggerProps = WithClassName<BaseTabs.Tab.Props>;

/**
 * 标签页按钮，value 对应内容面板
 * @example
 * ```tsx
 * <TabsTrigger value="overview">概览</TabsTrigger>
 * ```
 */
export function TabsTrigger({ className, ...props }: TabsTriggerProps) {
  return (
    <BaseTabs.Tab
      data-slot="tabs-trigger"
      className={cn(
        'relative z-10 inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-lg px-3 py-1.5 text-sm font-medium text-muted-foreground outline-none transition-colors duration-150 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring data-active:text-foreground data-disabled:pointer-events-none data-disabled:opacity-45 [&_svg]:size-4',
        className,
      )}
      {...props}
    />
  );
}

export type TabsContentProps = WithClassName<BaseTabs.Panel.Props>;

/**
 * 选中时显示的标签内容
 * @example
 * ```tsx
 * <TabsContent value="overview">概览内容</TabsContent>
 * ```
 */
export function TabsContent({ className, ...props }: TabsContentProps) {
  return (
    <BaseTabs.Panel
      data-slot="tabs-content"
      className={cn(
        'mt-4 min-w-0 outline-none focus-visible:ring-2 focus-visible:ring-ring data-[orientation=vertical]:mt-0 data-[orientation=vertical]:flex-1',
        className,
      )}
      {...props}
    />
  );
}
