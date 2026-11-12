import { Button, FieldDescription, FieldError } from '@hysilens/ui';
import { useMutation } from '@tanstack/react-query';
import { Download } from 'lucide-react';

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
    <section aria-labelledby="kugou-diagnostics-title" className="space-y-3">
      <h2 id="kugou-diagnostics-title" className="font-semibold">
        酷狗诊断
      </h2>
      <FieldDescription>
        播放出现问题的歌曲后导出报告。报告包含系统信息、应用与模块版本、文件哈希、播放状态和少量内存片段，连续采集约三秒，完成后可检查并分享。
      </FieldDescription>
      <Button
        type="button"
        variant="outline"
        size="sm"
        aria-label="导出酷狗诊断报告"
        isLoading={report.isPending}
        onClick={() => report.mutate()}
      >
        <Download className="size-3.5" />
        {report.isPending ? '正在导出…' : '导出诊断报告'}
      </Button>
      <p role="status" className="text-muted-foreground text-xs break-all">
        {message}
      </p>
      <FieldError show={Boolean(report.error)}>{report.error?.message}</FieldError>
    </section>
  );
}
