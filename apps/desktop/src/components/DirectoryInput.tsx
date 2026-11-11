import { Button, FieldError, Input } from '@hysilens/ui';
import { useMutation } from '@tanstack/react-query';
import { Folder } from 'kubai-icons-react';

interface Props {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
}

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
        <Input id={id} value={value} onChange={event => onChange(event.target.value)} />
        <Button
          type="button"
          variant="outline"
          size="icon"
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
