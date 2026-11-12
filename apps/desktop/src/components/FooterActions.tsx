import { createContext, useContext } from 'react';
import type { ReactNode } from 'react';
import { createPortal } from 'react-dom';

/**
 * 页面提供给面板级 footer 操作使用的 DOM 节点；挂载前为 `null`
 */
export const FooterActionsContext = createContext<HTMLDivElement | null>(null);

/**
 * 把面板操作 portal 到页面 footer 中，footer 出现之前不渲染任何内容
 */
export function FooterActions({ children }: { children: ReactNode }) {
  const target = useContext(FooterActionsContext);

  return target ? createPortal(children, target) : null;
}
