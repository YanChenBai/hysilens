import type { AppSettings } from '@hysilens/protocol';
import {
  Button,
  Dialog,
  DialogBody,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
  FieldError,
  Spinner,
  Switch,
} from '@hysilens/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { CircleCheck, ExternalLink, Rocket } from 'kubai-icons-react';
import { useEffect, useRef, useState } from 'react';

import { control } from '../lib/control';
import type { EnhancedStatus } from '../lib/control';
import { SettingsRow } from './SettingsSection';

const statusKey = ['kugou-enhanced'];

/**
 * 连接指示：`ok` 已连接，`warn` 需要用户处理或已回退，`idle` 等待中；关闭时为 `null`
 */
type Tone = 'ok' | 'warn' | 'idle' | null;

function describe(status: EnhancedStatus | undefined): { text: string; tone: Tone } {
  if (!status?.enabled) {
    return {
      text: '通过酷狗内嵌网页读取进度和歌曲，并联网查询歌词',
      tone: null,
    };
  }

  if (status.connected) {
    return { text: '已连接，正在使用增强模式', tone: 'ok' };
  }

  if (status.portOpen && !status.endpointAvailable) {
    return { text: '调试端口已占用或酷狗页面不可用，请检查后重试', tone: 'warn' };
  }

  if (status.endpointAvailable) {
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
 * 元素中心点，键盘打开对话框时作为展开原点
 */
function centerOf(element: Element | null): { x: number; y: number } | null {
  const rect = element?.getBoundingClientRect();

  return rect ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 } : null;
}

/**
 * 在操作位置展示创建进度和结果
 */
function ShortcutButton({
  pending,
  success,
  onCreate,
  variant = 'ghost',
  stacked = false,
}: {
  pending: boolean;

  success: boolean;

  onCreate: () => void;

  variant?: 'ghost' | 'secondary';

  stacked?: boolean;
}) {
  const iconSize = stacked ? 24 : 20;
  let label = '创建快捷方式';
  let icon = <ExternalLink color="#a78bfa" size={iconSize} trigger="parent" />;

  if (pending) {
    label = '正在创建…';
    icon = <Spinner className={stacked ? 'size-6' : undefined} />;
  } else if (success) {
    label = '已创建到桌面';
    icon = <CircleCheck color="#34d399" size={iconSize} trigger="parent" />;
  }

  return (
    <Button
      type="button"
      size="sm"
      variant={variant}
      disabled={pending}
      className={`${stacked ? 'h-16 w-full min-w-0 px-1' : ''} ${success ? 'text-success' : ''}`}
      title="在桌面创建增强模式快捷方式；创建后可双击启动酷狗，也可再次点击重建"
      onClick={onCreate}
    >
      <span
        role="status"
        aria-live="polite"
        aria-atomic="true"
        className={`inline-flex items-center ${stacked ? 'flex-col gap-2' : 'gap-1.5'}`}
      >
        {icon}
        {label}
      </span>
    </Button>
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
  const [origin, setOrigin] = useState<{ x: number; y: number } | null>(null);
  /*
   * Switch 的变更事件来自内部隐藏的 input，拿不到点击坐标；在开关上自行记录，
   * 让对话框与其他由 DialogTrigger 打开的对话框一样从点击处展开
   */
  const switchControl = useRef<HTMLElement>(null);
  const launchRequest = useRef(0);
  const pointer = useRef<{ x: number; y: number } | null>(null);

  useEffect(
    () => () => {
      launchRequest.current += 1;
    },
    [],
  );

  const status = useQuery<EnhancedStatus>({
    queryKey: statusKey,
    enabled: false,
  });

  /* 增强模式是运行时开关：只写入该项并即时生效，不会重启服务或刷新界面 */
  const toggle = useMutation({
    mutationFn: control.setKugou,
    onSuccess: (_, enabled) => {
      queries.setQueryData<AppSettings>(['settings'], old =>
        old ? { ...old, kugouCdp: enabled } : old,
      );
      queries.setQueryData<EnhancedStatus>(statusKey, old => (old ? { ...old, enabled } : old));
    },
  });

  const launch = useMutation({
    mutationFn: control.launchKugou,
  });

  const shortcut = useMutation({
    mutationFn: control.createKugouShortcut,
  });

  const { isSuccess: shortcutCreated, reset: resetShortcut } = shortcut;

  useEffect(() => {
    if (!shortcutCreated) {
      return;
    }

    const timer = window.setTimeout(resetShortcut, 3000);

    return () => window.clearTimeout(timer);
  }, [shortcutCreated, resetShortcut]);

  const enabled = status.data?.enabled ?? false;
  const canLaunch = enabled && status.data !== undefined && !status.data.endpointAvailable;
  const error = toggle.error ?? launch.error ?? shortcut.error;

  async function enableAndLaunch() {
    const request = ++launchRequest.current;

    await toggle.mutateAsync(true);

    if (request !== launchRequest.current) {
      return;
    }

    setOpen(false);
    launch.mutate();
  }

  return (
    <>
      <SettingsRow label="酷狗增强模式" description={<StatusText status={status.data} />}>
        <div className="flex items-center gap-2">
          <Switch
            ref={switchControl}
            aria-label="酷狗增强模式"
            checked={enabled}
            disabled={!status.data || toggle.isPending}
            onPointerDown={event => {
              pointer.current = { x: event.clientX, y: event.clientY };
            }}
            onKeyDown={() => {
              pointer.current = null;
            }}
            onCheckedChange={checked => {
              launch.reset();

              if (checked) {
                toggle.reset();
                shortcut.reset();
                setOrigin(pointer.current ?? centerOf(switchControl.current));
                setOpen(true);
              } else {
                toggle.mutate(false);
              }

              pointer.current = null;
            }}
          />
        </div>
      </SettingsRow>
      {enabled ? (
        <div className="border-border/50 grid grid-cols-2 items-start gap-1 border-t py-2">
          <ShortcutButton
            stacked
            pending={shortcut.isPending}
            success={shortcut.isSuccess}
            onCreate={() => shortcut.mutate()}
          />
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="h-16 w-full min-w-0 flex-col gap-2 px-1"
            disabled={!canLaunch}
            isLoading={launch.isPending}
            onClick={() => launch.mutate()}
          >
            <Rocket color="#60a5fa" size={24} trigger="parent" />
            启动酷狗
          </Button>
        </div>
      ) : null}
      {!open && error ? <FieldError show>{error.message}</FieldError> : null}
      <Dialog
        open={open}
        origin={origin}
        onOpenChange={nextOpen => {
          if (!nextOpen) {
            launchRequest.current += 1;
          }

          setOpen(nextOpen);
        }}
      >
        <DialogContent>
          <DialogTitle>启用酷狗增强模式</DialogTitle>
          <DialogBody>
            <DialogDescription>
              通过酷狗内嵌网页的接口读取播放进度和歌曲
              hash，读取失败时自动回退到当前方式。该接口并非酷狗公开承诺的功能，酷狗升级后可能失效。
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
              <div>
                <p className="text-foreground font-medium">联网查询歌词</p>
                <p className="mt-1">
                  每首歌会向酷狗歌词服务（krcs.kugou.com、lyrics.kugou.com）发送当前歌曲的
                  hash、曲库 ID
                  和时长，取得酷狗推荐的歌词：用于准确定位本地缓存，本地缓存匹配不到时直接使用下载的歌词。不发送账号或设备信息，下载的歌词不会写入磁盘。
                </p>
              </div>
              <p className="rounded-xl bg-amber-500/10 px-3 py-2 text-amber-700 dark:text-amber-300">
                酷狗会在 127.0.0.1:{status.data?.port ?? 9225}{' '}
                开放调试端口，仅本机可访问，但本机其他程序也能借此操作已登录的酷狗页面。关闭增强模式后，需正常重启酷狗端口才会关闭。
              </p>
              <FieldError show={Boolean(toggle.error ?? shortcut.error)}>
                {(toggle.error ?? shortcut.error)?.message}
              </FieldError>
            </div>
          </DialogBody>
          <DialogFooter className="flex-wrap">
            <DialogClose render={<Button variant="outline" size="sm" />}>取消</DialogClose>
            <ShortcutButton
              variant="secondary"
              pending={shortcut.isPending}
              success={shortcut.isSuccess}
              onCreate={() => shortcut.mutate()}
            />
            <Button
              size="sm"
              isLoading={toggle.isPending}
              onClick={() => void enableAndLaunch().catch(() => {})}
            >
              <Rocket color="#60a5fa" size={20} trigger="parent" />
              启用并启动酷狗
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
