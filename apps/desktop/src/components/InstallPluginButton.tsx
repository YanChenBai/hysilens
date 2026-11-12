import type { PluginManifest } from '@hysilens/plugin';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { PackagePlus } from 'kubai-icons-react';
import { useRef } from 'react';

import { installPlugin } from '../lib/api';
import { IconButton } from './IconButton';

/**
 * 选择一个 `hysilens pack` zip 并安装到用户 plugin 目录
 *
 * @param onInstalled 安装成功后以校验过的 manifest 调用
 * @param onError 上传失败时以可读消息调用
 */
export function InstallPluginButton({
  onInstalled,
  onError,
}: {
  onInstalled: (manifest: PluginManifest) => void;

  onError: (message: string) => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const queries = useQueryClient();

  const install = useMutation({
    mutationFn: installPlugin,

    onSuccess: manifest => {
      /* socket 也会广播该事件；这里重新获取可让本窗口始终同步 */
      void queries.invalidateQueries({ queryKey: ['plugins'] });
      void queries.invalidateQueries({ queryKey: ['plugin-config', manifest.id] });
      void queries.invalidateQueries({ queryKey: ['config-editor', manifest.id] });
      onInstalled(manifest);
    },

    onError: error => onError(error instanceof Error ? error.message : String(error)),
  });

  return (
    <>
      <input
        ref={input}
        type="file"
        accept=".zip,application/zip"
        className="hidden"
        aria-hidden="true"
        tabIndex={-1}
        onChange={event => {
          const file = event.target.files?.[0];

          /* 重置后再次选择同一文件仍会触发 change */
          event.target.value = '';

          if (file) {
            install.mutate(file);
          }
        }}
      />
      <IconButton
        label="安装插件（hysilens pack 生成的 zip）"
        isLoading={install.isPending}
        onClick={() => input.current?.click()}
      >
        <PackagePlus color="#a78bfa" size={20} trigger="parent" />
      </IconButton>
    </>
  );
}
