import { afterEach, beforeEach, describe, expect, it, vi } from 'vite-plus/test';

import { control, hasControlIpc, installPlugin, subscribeControl } from './control';
import type { ControlEvent } from './control';

const { invoke, bridge, channels } = vi.hoisted(() => ({
  invoke: vi.fn(),
  bridge: vi.fn(),
  channels: [] as ((event: ControlEvent) => void)[],
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke,
  Channel: class {
    constructor(receive: (event: ControlEvent) => void) {
      channels.push(receive);
    }
  },
}));

vi.mock('./local-overlay', () => ({ getDesktopBridge: bridge }));

beforeEach(() => {
  vi.clearAllMocks();
  channels.length = 0;
  const main = {};
  vi.stubGlobal('window', Object.assign(main, { top: main }));
  bridge.mockReturnValue({ controlIpc: true });
});

afterEach(() => vi.unstubAllGlobals());

describe('desktop control transport', () => {
  it('requires the top frame and an explicit host capability', () => {
    expect(hasControlIpc()).toBe(true);
    bridge.mockReturnValue({});
    expect(hasControlIpc()).toBe(false);
    bridge.mockReturnValue({ controlIpc: true });
    vi.stubGlobal('window', { top: {} });
    expect(hasControlIpc()).toBe(false);
  });

  it('sends typed operations through IPC without HTTP requests', async () => {
    const fetch = vi.fn();
    vi.stubGlobal('fetch', fetch);
    invoke.mockResolvedValue({ plugins: [], errors: [] });

    await control.removePlugin('external');

    expect(invoke).toHaveBeenCalledWith('control_request', {
      request: { method: 'pluginsRemove', id: 'external' },
    });
    expect(fetch).not.toHaveBeenCalled();
  });

  it('does not replay a failed desktop mutation over HTTP', async () => {
    const fetch = vi.fn();
    vi.stubGlobal('fetch', fetch);
    invoke.mockRejectedValue('内置插件不能删除');

    await expect(control.removePlugin('builtin:scroll')).rejects.toThrow('内置插件不能删除');
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(fetch).not.toHaveBeenCalled();
  });

  it('rejects control calls outside the desktop instead of using HTTP', async () => {
    bridge.mockReturnValue(undefined);
    const fetch = vi.fn();
    vi.stubGlobal('fetch', fetch);

    await expect(control.setKugou(true)).rejects.toThrow('桌面主窗口');
    expect(invoke).not.toHaveBeenCalled();
    expect(fetch).not.toHaveBeenCalled();
  });

  it('rejects browser subscriptions before creating a native channel', () => {
    bridge.mockReturnValue(undefined);
    const onError = vi.fn();

    const release = subscribeControl(vi.fn(), onError);
    release();

    expect(channels).toHaveLength(0);
    expect(invoke).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledWith(
      expect.objectContaining({ message: expect.stringContaining('桌面主窗口') }),
    );
  });

  it('passes plugin archives as a binary IPC body', async () => {
    invoke.mockResolvedValue({ id: 'example' });
    const file = new Blob([new Uint8Array([80, 75, 3, 4])]);

    await installPlugin(file);

    const bytes = invoke.mock.calls[0][1];
    expect(invoke.mock.calls[0][0]).toBe('control_install_plugin');
    expect(bytes).toBeInstanceOf(ArrayBuffer);
    expect([...new Uint8Array(bytes)]).toEqual([80, 75, 3, 4]);
  });
});

describe('desktop control subscriptions', () => {
  it('releases a subscription that finishes opening after cleanup', async () => {
    const receive = vi.fn();
    const onError = vi.fn();
    let complete!: (id: number) => void;
    invoke.mockImplementation((command: string) =>
      command === 'control_subscribe'
        ? new Promise<number>(resolve => {
            complete = resolve;
          })
        : Promise.resolve(),
    );

    const release = subscribeControl(receive, onError);
    release();
    channels[0]({ kind: 'connection', connected: true });
    complete(42);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('control_unsubscribe', { id: 42 }));

    expect(receive).not.toHaveBeenCalled();
    expect(onError).not.toHaveBeenCalled();
  });

  it('keeps resync and connection events and stops delivery after cleanup', async () => {
    invoke.mockResolvedValue(7);
    const receive = vi.fn();
    const release = subscribeControl(receive, vi.fn());
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    await Promise.resolve();

    channels[0]({ kind: 'connection', connected: false });
    channels[0]({ kind: 'resync' });
    release();
    channels[0]({ kind: 'connection', connected: true });

    expect(receive.mock.calls).toEqual([
      [{ kind: 'connection', connected: false }],
      [{ kind: 'resync' }],
    ]);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('control_unsubscribe', { id: 7 }));
  });
});
