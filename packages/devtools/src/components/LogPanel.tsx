import { cn } from 'cn';
import { Undo } from 'kubai-icons-react';

import type { LogEntry } from '../lib/use-plugin-host';
import { IconButton } from './IconButton';

const LEVEL_CLASS: Record<LogEntry['level'], string> = {
  info: 'text-muted-foreground',
  warn: 'text-amber-600 dark:text-amber-400',
  error: 'text-destructive',
};

/**
 * Host 生命周期变化，以及 plugin 通过 `report()` 上报的错误
 */
export function LogPanel({ log, onClear }: { log: LogEntry[]; onClear: () => void }) {
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      <div className="flex items-center justify-between px-4">
        <p className="text-muted-foreground text-xs">宿主状态变化与插件 report() 上报的错误。</p>
        <IconButton label="清空日志" disabled={!log.length} onClick={onClear}>
          <Undo color="#94a3b8" size={18} trigger="parent" />
        </IconButton>
      </div>
      <ol className="min-h-0 flex-1 space-y-1 overflow-y-auto px-4 pb-4 font-mono text-xs">
        {log.length ? null : <li className="text-muted-foreground font-sans text-sm">暂无记录</li>}
        {[...log].reverse().map(entry => (
          <li key={entry.id} className={cn('break-words', LEVEL_CLASS[entry.level])}>
            <span className="text-muted-foreground mr-2 tabular-nums">
              {entry.at.toLocaleTimeString()}
            </span>
            {entry.message}
          </li>
        ))}
      </ol>
    </div>
  );
}
