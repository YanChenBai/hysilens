import { FieldError, Label, Switch } from '@hysilens/ui';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { getDesktopBridge, useDesktopBridge } from '../lib/local-overlay';
import type { LocalOverlaySettings } from '../lib/local-overlay';

const queryKey = ['local-overlay'];

export function LocalOverlayControls() {
  const desktopBridge = useDesktopBridge();

  const settings = useQuery({
    queryKey,
    enabled: Boolean(desktopBridge),
    queryFn: () => desktopBridge!.getOverlay(),
  });

  if (!desktopBridge) {
    return null;
  }

  if (!settings.data) {
    return settings.error ? <FieldError show>{String(settings.error)}</FieldError> : null;
  }

  return <OverlayControls initial={settings.data} />;
}

function OverlayControls({ initial }: { initial: LocalOverlaySettings }) {
  const queries = useQueryClient();

  const save = useMutation({
    mutationFn: ({ enabled, clickThrough }: LocalOverlaySettings) =>
      getDesktopBridge()!.setOverlay({ enabled, clickThrough }),
    onSuccess: settings => {
      queries.setQueryData(queryKey, settings);
    },
  });

  return (
    <section className="space-y-3" aria-labelledby="local-overlay-heading">
      <div className="flex items-center justify-between gap-4">
        <h2 id="local-overlay-heading" className="font-semibold">
          本地悬浮歌词
        </h2>
        <Switch
          aria-label="显示本地悬浮歌词"
          checked={initial.enabled}
          disabled={save.isPending}
          onCheckedChange={enabled => save.mutate({ ...initial, enabled })}
        />
      </div>
      <div className="flex items-center justify-between gap-4">
        <Label htmlFor="overlay-click-through">鼠标穿透</Label>
        <Switch
          id="overlay-click-through"
          checked={initial.clickThrough}
          disabled={save.isPending}
          onCheckedChange={clickThrough => save.mutate({ ...initial, clickThrough })}
        />
      </div>
      <FieldError show={Boolean(save.error)}>{save.error ? String(save.error) : ''}</FieldError>
    </section>
  );
}
