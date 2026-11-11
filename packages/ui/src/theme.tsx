import { createStore, useSelector } from '@tanstack/react-store';
import { MotionConfig } from 'motion/react';
import type { PropsWithChildren } from 'react';
import { createContext, useContext, useEffect, useState } from 'react';

export type Theme = 'light' | 'dark' | 'system';
const createThemeStore = (theme: Theme) => createStore({ theme });
const ThemeContext = createContext<ReturnType<typeof createThemeStore> | null>(null);

export type ThemeProviderProps = PropsWithChildren<{
  /** 初始主题；默认跟随系统，挂载后优先读取本地保存值。 */
  defaultTheme?: Theme;

  /** 保存主题偏好的 localStorage 键；默认 hysilens-theme。 */
  storageKey?: string;
}>;

/**
 * 提供独立的主题状态，并统一遵从系统的减少动画设置。
 * @example
 * ```tsx
 * <ThemeProvider defaultTheme="system">
 *   <App />
 * </ThemeProvider>
 * ```
 */
export function ThemeProvider({
  children,
  defaultTheme = 'system',
  storageKey = 'hysilens-theme',
}: ThemeProviderProps) {
  // 每个 Provider 持有独立 store，避免 SSR 请求或多个预览实例共享状态。
  const [store] = useState(() => createThemeStore(defaultTheme));

  useEffect(() => {
    try {
      const saved = localStorage.getItem(storageKey);

      if (saved === 'light' || saved === 'dark' || saved === 'system') {
        store.setState(() => ({ theme: saved }));
      }
    } catch {
      /* 隐私模式下仍允许在当前会话内切换主题。 */
    }

    const media = window.matchMedia('(prefers-color-scheme: dark)');

    const applyTheme = () => {
      const theme = store.get().theme;
      document.documentElement.classList.toggle(
        'dark',
        theme === 'dark' || (theme === 'system' && media.matches),
      );
    };

    const subscription = store.subscribe(() => {
      applyTheme();

      try {
        localStorage.setItem(storageKey, store.get().theme);
      } catch {
        /* 存储不可用时保留内存状态。 */
      }
    });

    applyTheme();
    media.addEventListener('change', applyTheme);

    return () => {
      subscription.unsubscribe();
      media.removeEventListener('change', applyTheme);
    };
  }, [store, storageKey]);

  return (
    <ThemeContext value={store}>
      <MotionConfig reducedMotion="user">{children}</MotionConfig>
    </ThemeContext>
  );
}

/**
 * 读取和切换所在 ThemeProvider 的主题。
 * @example
 * ```tsx
 * const { theme, setTheme } = useTheme();
 * setTheme('dark');
 * ```
 */
export function useTheme() {
  const store = useContext(ThemeContext);

  if (!store) {
    throw new Error('useTheme 必须在 ThemeProvider 内使用');
  }

  const theme = useSelector(store, state => state.theme);

  return { theme, setTheme: (nextTheme: Theme) => store.setState(() => ({ theme: nextTheme })) };
}
