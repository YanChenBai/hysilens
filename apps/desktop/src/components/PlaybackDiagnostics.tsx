import type { PlaybackSource } from '@hysilens/protocol';
import {
  Button,
  Dialog,
  DialogBody,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
  DialogTrigger,
  FieldError,
  Spinner,
} from '@hysilens/ui';
import { useMutation } from '@tanstack/react-query';
import { Activity } from 'kubai-icons-react';
import { Download } from 'lucide-react';
import { useState } from 'react';

import { getJson } from '../lib/api';
import { SettingsRow, SettingsSection } from './SettingsSection';

interface DiagnosticInfo {
  source: PlaybackSource | null;

  name: string;

  description: string;
}

/**
 * 先展示适配器提供的采集说明，再按确认的平台导出报告
 */
export function PlaybackDiagnostics() {
  const [open, setOpen] = useState(false);

  const target = useMutation({
    mutationFn: () => getJson<DiagnosticInfo>('/api/diagnostics'),
  });

  const report = useMutation({
    mutationFn: async (info: DiagnosticInfo) => {
      const response = await fetch('/api/diagnostics/export', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ source: info.source }),
      });

      if (!response.ok) {
        throw new Error(await response.text());
      }

      return response.json() as Promise<{ path: string | null }>;
    },
  });

  let message = '';

  if (report.isPending) {
    message = `正在导出${report.variables.name}报告：请选择保存位置，并保持异常歌曲播放，等待采集完成。`;
  } else if (report.data?.path) {
    message = `已导出：${report.data.path}`;
  } else if (report.data) {
    message = '已取消导出。';
  }

  return (
    <SettingsSection title="播放诊断" icon={<Activity size={20} color="#fb923c" />}>
      <SettingsRow label="诊断报告" description="自动选择当前播放器的专属报告">
        <Dialog
          open={open}
          onOpenChange={nextOpen => {
            setOpen(nextOpen);

            if (nextOpen) {
              target.mutate();
            }
          }}
        >
          <DialogTrigger
            render={<Button type="button" variant="secondary" size="sm" />}
            aria-label="导出播放诊断报告"
            aria-busy={report.isPending}
            disabled={report.isPending}
          >
            {report.isPending ? (
              <Spinner className="size-3.5" />
            ) : (
              <Download className="size-3.5" aria-hidden="true" />
            )}
            {report.isPending ? '正在导出…' : '导出报告'}
          </DialogTrigger>
          <DialogContent>
            <DialogTitle>导出播放诊断报告</DialogTitle>
            <DialogBody>
              <DialogDescription>
                先确认采集内容，再选择保存位置。报告仅保存到本地，不会自动上传。
              </DialogDescription>
              <div className="mt-5 space-y-4 text-sm leading-relaxed">
                {target.isPending ? (
                  <p role="status">正在识别诊断平台…</p>
                ) : (
                  <DiagnosticDetails info={target.data} />
                )}
                <FieldError show={Boolean(target.error)}>{target.error?.message}</FieldError>
              </div>
            </DialogBody>
            <DialogFooter>
              <DialogClose render={<Button variant="outline" size="sm" />}>取消</DialogClose>
              <Button
                size="sm"
                disabled={!target.isSuccess || target.isPending || report.isPending}
                onClick={() => {
                  if (target.data) {
                    report.mutate(target.data);
                    setOpen(false);
                  }
                }}
              >
                开始采集并导出
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </SettingsRow>
      <p
        role="status"
        className="text-muted-foreground py-2 text-xs leading-relaxed break-all empty:hidden"
      >
        {message}
      </p>
      <FieldError show={Boolean(report.error)} className="py-2">
        {report.error?.message}
      </FieldError>
    </SettingsSection>
  );
}

function DiagnosticDetails({ info }: { info: DiagnosticInfo | undefined }) {
  if (!info) {
    return null;
  }

  return (
    <>
      <div>
        <p className="font-medium">本次报告：{info.name}</p>
        <p className="text-muted-foreground mt-1">{info.description}。</p>
      </div>
      <div>
        <p className="font-medium">采集前请准备</p>
        <p className="text-muted-foreground mt-1">
          {info.source
            ? '请在对应播放器中播放出现异常的歌曲，并在采集完成前保持播放，避免暂停、拖动进度、切歌或退出播放器。连续采集约三秒，内存定位可能需要更久。'
            : '未识别到播放器也可以导出基础报告。如果希望采集专属数据，请先打开播放器播放歌曲，再重新打开此弹窗。'}
        </p>
      </div>
      <div className="rounded-xl border border-amber-500/25 bg-amber-500/5 p-3">
        <p className="font-medium">分享前请检查报告</p>
        <p className="text-muted-foreground mt-1">
          报告包含应用与系统信息、歌曲信息、歌词匹配状态和错误；依平台不同，还可能包含本地路径、歌曲标识、文件哈希或少量内存片段，可能暴露个人信息。请先检查内容，再决定是否分享。
        </p>
      </div>
    </>
  );
}
