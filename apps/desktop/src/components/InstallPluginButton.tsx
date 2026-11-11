import type { PluginManifest } from '@hysilens/plugin';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { PackagePlus } from 'kubai-icons-react';
import { useRef } from 'react';

import { installPlugin } from '../lib/api';
import { IconButton } from './IconButton';

/** Pick a `hysilens pack` zip and install it into the user plugin directory. */
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
      // The socket also announces it; refetching here keeps this window in step regardless.
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
          // Reset so choosing the same file again still triggers a change.
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
