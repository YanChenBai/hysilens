import { Button, FieldError } from '@hysilens/ui';
import { useMutation } from '@tanstack/react-query';
import { Folder } from 'kubai-icons-react';

import { control } from '../lib/control';

/**
 * 请求 host 在系统文件管理器中打开用户 plugin 目录
 */
export function OpenPluginDirectoryButton() {
  const open = useMutation({
    mutationFn: control.openPluginDirectory,
  });

  return (
    <div className="min-w-0">
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="h-16 w-full min-w-0 flex-col gap-2 px-1"
        aria-label="打开插件目录"
        isLoading={open.isPending}
        onClick={() => open.mutate()}
      >
        <Folder color="#d4ac64" size={24} trigger="parent" />
        打开目录
      </Button>
      <FieldError show={Boolean(open.error)}>{open.error?.message}</FieldError>
    </div>
  );
}
