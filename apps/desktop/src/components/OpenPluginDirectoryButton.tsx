import { FieldError } from '@hysilens/ui';
import { useMutation } from '@tanstack/react-query';
import { Folder } from 'kubai-icons-react';

import { IconButton } from './IconButton';

/**
 * 请求 host 在系统文件管理器中打开用户 plugin 目录
 */
export function OpenPluginDirectoryButton() {
  const open = useMutation({
    mutationFn: async () => {
      const response = await fetch('/api/plugins/directory', { method: 'POST' });

      if (!response.ok) {
        throw new Error(await response.text());
      }
    },
  });

  return (
    <div>
      <IconButton label="打开插件目录" isLoading={open.isPending} onClick={() => open.mutate()}>
        <Folder color="#d4ac64" size={20} trigger="parent" />
      </IconButton>
      <FieldError show={Boolean(open.error)}>{open.error?.message}</FieldError>
    </div>
  );
}
