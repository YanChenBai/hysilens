import { Button, FieldError } from '@hysilens/ui';
import { Copy } from 'kubai-icons-react';
import { Check } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';

export function CopySourceButton({ url }: { url: string }) {
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState('');
  const resetTimer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(resetTimer.current), []);

  async function copy() {
    try {
      await navigator.clipboard.writeText(url);
      window.clearTimeout(resetTimer.current);
      setCopied(true);
      setError('');
      resetTimer.current = window.setTimeout(() => setCopied(false), 2000);
    } catch (error) {
      setError(String(error));
    }
  }

  return (
    <div>
      <Button
        variant="ghost"
        size="sm"
        aria-label={copied ? '已复制 OBS 地址' : '复制 OBS 地址'}
        title={copied ? '已复制 OBS 地址' : '复制 OBS 地址'}
        onClick={copy}
      >
        {copied ? (
          <Check className="text-success size-4" />
        ) : (
          <Copy className="size-4" color="#60a5fa" trigger="parent" />
        )}
        {copied ? '已复制' : '复制 OBS 地址'}
      </Button>
      <FieldError show={Boolean(error)}>{error}</FieldError>
    </div>
  );
}
