import type { PluginManifest } from '@hysilens/plugin';
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
} from '@hysilens/ui';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Trash } from 'kubai-icons-react';
import { useState } from 'react';

import { control } from '../lib/control';

/**
 * 确认后删除所选外部 plugin，内置 plugin 不提供此入口
 */
export function RemovePluginButton({ plugin }: { plugin: PluginManifest }) {
  const [open, setOpen] = useState(false);
  const queries = useQueryClient();

  const remove = useMutation({
    mutationFn: () => control.removePlugin(plugin.id),
    onSuccess: catalog => {
      setOpen(false);
      queries.removeQueries({ queryKey: ['plugin-config', plugin.id] });
      queries.removeQueries({ queryKey: ['config-editor', plugin.id] });
      queries.setQueryData(['plugins'], catalog);
    },
  });

  return (
    <Dialog
      open={open}
      onOpenChange={nextOpen => {
        if (remove.isPending) {
          return;
        }

        setOpen(nextOpen);
        remove.reset();
      }}
    >
      <DialogTrigger
        render={
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="text-destructive h-16 w-full min-w-0 flex-col gap-2 px-1"
          />
        }
        aria-label={`删除插件 ${plugin.name}`}
      >
        <Trash color="#f87171" size={24} trigger="parent" aria-hidden="true" />
        删除插件
      </DialogTrigger>
      <DialogContent>
        <DialogTitle>删除「{plugin.name}」？</DialogTitle>
        <DialogBody>
          <DialogDescription>
            删除后将切回默认插件。已保存的插件配置会保留，重新安装后可继续使用。
            如果这是开发链接，只移除链接，不删除源码。
          </DialogDescription>
          <FieldError show={Boolean(remove.error)} className="mt-3">
            {remove.error?.message}
          </FieldError>
        </DialogBody>
        <DialogFooter>
          <DialogClose render={<Button variant="outline" size="sm" disabled={remove.isPending} />}>
            取消
          </DialogClose>
          <Button
            variant="destructive"
            size="sm"
            isLoading={remove.isPending}
            onClick={() => remove.mutate()}
          >
            删除插件
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
