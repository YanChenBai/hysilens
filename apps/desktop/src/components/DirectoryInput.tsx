import { Button, FieldError, Input } from '@hysilens/ui';
import { useMutation } from '@tanstack/react-query';
import { Folder } from 'kubai-icons-react';

interface Props {
  id: string;

  /**
   * 用在浏览按钮无障碍标签中的字段名
   */
  label: string;

  value: string;

  onChange: (value: string) => void;
}

/**
 * 目录路径文本输入框，带原生文件夹选择按钮
 */
export function DirectoryInput({ id, label, value, onChange }: Props) {
  const choose = useMutation({
    mutationFn: async () => {
      const response = await fetch('/api/dialog/directory', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ directory: value }),
      });

      if (!response.ok) {
        throw new Error(await response.text());
      }

      return response.json() as Promise<{ path: string | null }>;
    },
    onSuccess: result => {
      if (result.path !== null) {
        onChange(result.path);
      }
    },
  });

  return (
    <div>
      <div className="flex items-center gap-2">
        <Input
          id={id}
          className="settings-input min-w-0"
          value={value}
          onChange={event => onChange(event.target.value)}
        />
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-9 shrink-0 rounded-xl"
          aria-label={`选择${label}`}
          title="选择文件夹"
          isLoading={choose.isPending}
          onClick={() => choose.mutate()}
        >
          <Folder size={20} color="#e9b44c" trigger="parent" />
        </Button>
      </div>
      <FieldError show={Boolean(choose.error)}>{choose.error?.message}</FieldError>
    </div>
  );
}
