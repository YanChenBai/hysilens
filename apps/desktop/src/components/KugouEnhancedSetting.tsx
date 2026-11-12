import type { AppSettings } from '@hysilens/protocol';
import {
  Button,
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
  FieldError,
  Switch,
} from '@hysilens/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';

import { getJson, postJson, putJson } from '../lib/api';
import { SettingsRow } from './SettingsSection';

/**
 * `/api/kugou/enhanced` 返回的增强模式状态
 */
interface EnhancedStatus {
  enabled: boolean;

  port: number;

  /**
   * 酷狗调试端口是否在本机监听
   */
  portOpen: boolean;

  kugouRunning: boolean;

  /**
   * 最近一次采样是否成功读到播放信息
   */
  connected: boolean;

  error: string | null;
}

const statusKey = ['kugou-enhanced'];

/**
 * 连接指示：`ok` 已连接，`warn` 需要用户处理或已回退，`idle` 等待中；关闭时为 `null`
 */
type Tone = 'ok' | 'warn' | 'idle' | null;

function describe(status: EnhancedStatus | undefined): { text: string; tone: Tone } {
  if (!status?.enabled) {
    return { text: '通过酷狗内嵌网页读取进度和歌曲，酷狗升级后通常仍可用', tone: null };
  }

  if (status.connected) {
    return { text: '已连接，正在使用增强模式', tone: 'ok' };
  }

  if (status.portOpen) {
    return status.error
      ? { text: `暂不可用，已回退内存读取：${status.error}`, tone: 'warn' }
      : { text: '正在连接酷狗…', tone: 'idle' };
  }

  if (status.kugouRunning) {
    return { text: '酷狗正以普通模式运行，请从托盘完全退出酷狗后再启动', tone: 'warn' };
  }

  return { text: '酷狗未运行，启动后自动连接', tone: 'idle' };
}

const toneClass = {
  ok: 'bg-success',
  warn: 'bg-amber-500',
  idle: 'bg-muted-foreground/60',
};

/**
 * 带连接指示点的状态说明
 */
function StatusText({ status }: { status: EnhancedStatus | undefined }) {
  const { text, tone } = describe(status);

  if (!tone) {
    return text;
  }

  return (
    <span className="inline-flex max-w-full items-baseline gap-1.5">
      <span
        aria-hidden="true"
        className={`relative top-[-1px] size-1.5 shrink-0 self-center rounded-full ${toneClass[tone]}`}
      />
      <span className="min-w-0 wrap-anywhere">{text}</span>
    </span>
  );
}

/**
 * 播放设置中的酷狗增强模式开关
 *
 * 开启前用模态框说明启动条件与本机调试端口的风险；关闭时直接停用，不影响正在运行的酷狗
 */
export function KugouEnhancedSetting() {
  const queries = useQueryClient();
  const [open, setOpen] = useState(false);
  const [notice, setNotice] = useState('');

  const status = useQuery({
    queryKey: statusKey,
    queryFn: () => getJson<EnhancedStatus>('/api/kugou/enhanced'),
    refetchInterval: 2000,
  });

  // 增强模式是运行时开关：只写入该项并即时生效，不会重启服务或刷新界面
  const toggle = useMutation({
    mutationFn: (enabled: boolean) => putJson('/api/kugou/enhanced', { enabled }),
    onSuccess: (_, enabled) => {
      queries.setQueryData<AppSettings>(['settings'], old =>
        old ? { ...old, kugouCdp: enabled } : old,
      );
      queries.setQueryData<EnhancedStatus>(statusKey, old => (old ? { ...old, enabled } : old));
      void queries.invalidateQueries({ queryKey: statusKey });
    },
  });

  const launch = useMutation({
    mutationFn: () => postJson<{ result: string }>('/api/kugou/enhanced/launch'),
    onSettled: () => void queries.invalidateQueries({ queryKey: statusKey }),
  });

  const shortcut = useMutation({
    mutationFn: () => postJson<{ path: string }>('/api/kugou/enhanced/shortcut'),
    onSuccess: ({ path }) => setNotice(`已在桌面创建快捷方式：${path}`),
  });

  const enabled = status.data?.enabled ?? false;
  const canLaunch = enabled && status.data !== undefined && !status.data.portOpen;
  const error = toggle.error ?? launch.error ?? shortcut.error;

  async function enableAndLaunch() {
    setNotice('');
    await toggle.mutateAsync(true);
    setOpen(false);
    launch.mutate();
  }

  return (
    <>
      <SettingsRow
        label="酷狗增强模式"
        description={
          <>
            <StatusText status={status.data} />
            {enabled ? (
              <span className="mt-1.5 flex flex-wrap items-center gap-1.5">
                {canLaunch ? (
                  <Button
                    size="sm"
                    variant="secondary"
                    className="h-6 rounded-md px-2 text-[11px]"
                    isLoading={launch.isPending}
                    onClick={() => launch.mutate()}
                  >
                    启动酷狗
                  </Button>
                ) : null}
                <Button
                  size="sm"
                  variant="secondary"
                  className="h-6 rounded-md px-2 text-[11px]"
                  isLoading={shortcut.isPending}
                  title="在桌面创建以增强模式启动酷狗的快捷方式，误删后可重建"
                  onClick={() => {
                    setNotice('');
                    shortcut.mutate();
                  }}
                >
                  创建快捷方式
                </Button>
              </span>
            ) : null}
            {enabled && !open && notice ? (
              <span className="mt-1 block wrap-anywhere">{notice}</span>
            ) : null}
          </>
        }
      >
        <div className="flex items-center gap-2">
          <Switch
            aria-label="酷狗增强模式"
            checked={enabled}
            disabled={!status.data || toggle.isPending}
            onCheckedChange={checked => {
              launch.reset();

              if (checked) {
                toggle.reset();
                shortcut.reset();
                setNotice('');
                setOpen(true);
              } else {
                toggle.mutate(false);
              }
            }}
          />
        </div>
      </SettingsRow>
      {!open && error ? <FieldError show>{error.message}</FieldError> : null}
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogTitle>启用酷狗增强模式</DialogTitle>
          <DialogDescription>
            通过酷狗内嵌网页的接口读取播放进度和歌曲
            hash，不依赖酷狗的内部版本，读取失败时自动回退到当前方式。
          </DialogDescription>
          <div className="text-muted-foreground mt-4 space-y-3 text-sm leading-relaxed">
            <div>
              <p className="text-foreground font-medium">使用前需要</p>
              <ul className="mt-1 list-disc space-y-0.5 pl-5">
                <li>从托盘完全退出酷狗，只关闭窗口无效</li>
                <li>通过下方按钮或增强快捷方式启动酷狗</li>
                <li>关闭酷狗的开机自启，否则开机启动的酷狗不会开启增强模式</li>
              </ul>
            </div>
            <p className="rounded-xl bg-amber-500/10 px-3 py-2 text-amber-700 dark:text-amber-300">
              酷狗会在 127.0.0.1:{status.data?.port ?? 9225}{' '}
              开放调试端口，仅本机可访问，但本机其他程序也能借此操作已登录的酷狗页面。关闭增强模式后，需正常重启酷狗端口才会关闭。
            </p>
            {notice ? <p className="text-foreground wrap-anywhere">{notice}</p> : null}
            <FieldError show={Boolean(toggle.error ?? shortcut.error)}>
              {(toggle.error ?? shortcut.error)?.message}
            </FieldError>
          </div>
          <DialogFooter className="flex-wrap">
            <DialogClose render={<Button variant="ghost" size="sm" />}>取消</DialogClose>
            <Button
              variant="secondary"
              size="sm"
              isLoading={shortcut.isPending}
              onClick={() => {
                setNotice('');
                shortcut.mutate();
              }}
            >
              创建快捷方式
            </Button>
            <Button
              size="sm"
              isLoading={toggle.isPending}
              onClick={() => void enableAndLaunch().catch(() => {})}
            >
              启用并启动酷狗
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
