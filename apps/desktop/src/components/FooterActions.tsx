import { createContext, useContext } from 'react';
import type { ReactNode } from 'react';
import { createPortal } from 'react-dom';

export const FooterActionsContext = createContext<HTMLDivElement | null>(null);

export function FooterActions({ children }: { children: ReactNode }) {
  const target = useContext(FooterActionsContext);

  return target ? createPortal(children, target) : null;
}
