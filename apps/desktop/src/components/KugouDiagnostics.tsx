import { Button, FieldDescription, FieldError } from '@hysilens/ui';
import { useMutation } from '@tanstack/react-query';
import { Activity } from 'kubai-icons-react';
import { Download } from 'lucide-react';

import { SettingsRow, SettingsSection } from './SettingsSection';

/**
 * 把酷狗播放诊断报告导出到用户选择的文件
 */
export function KugouDiagnostics() {
  const report = useMutation({
    mutationFn: async () => {
      const response = await fetch('/api/diagnostics/kugou/export', { method: 'POST' });

      if (!response.ok) {
        throw new Error(await response.text());
      }

      return response.json() as Promise<{ path: string | null }>;
    },
  });

  let message = '';

  if (report.isPending) {
    message = '请选择保存位置，并保持酷狗播放，等待采集完成。';
  } else if (report.data?.path) {
    message = `已导出：${report.data.path}`;
  } else if (report.data) {
    message = '已取消导出。';
  }

  return (
    <SettingsSection title="酷狗诊断" icon={<Activity size={20} color="#fb923c" />}>
      <SettingsRow label="诊断报告" description="播放异常歌曲后导出，采集约三秒">
        <Button
          type="button"
          variant="secondary"
          size="sm"
          aria-label="导出酷狗诊断报告"
          isLoading={report.isPending}
          onClick={() => report.mutate()}
        >
          <Download className="size-3.5" />
          {report.isPending ? '正在导出…' : '导出报告'}
        </Button>
      </SettingsRow>
      <details className="text-muted-foreground py-1 text-xs">
        <summary className="cursor-pointer">报告包含哪些信息</summary>
        <FieldDescription className="mt-2">
          报告包含系统信息、应用与模块版本、文件哈希、播放状态和少量内存片段。连续采集约三秒，完成后可检查并分享。
        </FieldDescription>
      </details>
      <p role="status" className="text-muted-foreground text-xs break-all empty:hidden">
        {message}
      </p>
      <FieldError show={Boolean(report.error)}>{report.error?.message}</FieldError>
    </SettingsSection>
  );
}
