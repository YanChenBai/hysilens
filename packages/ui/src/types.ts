/**
 * 保留原始属性，仅将 Base UI 的 `className` 回调收窄为样式字符串
 * @example
 * ```ts
 * type TabsTriggerProps = WithClassName<BaseTabs.Tab.Props>;
 * ```
 */
export type WithClassName<Props> = Omit<Props, 'className'> & {
  /**
   * 追加到组件根节点的样式类
   */
  className?: string;
};
