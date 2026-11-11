import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

import { describe, expect, it, vi } from 'vite-plus/test';

const script = readFileSync(
  new URL('../../src-tauri/src/overlay-drag.js', import.meta.url),
  'utf8',
);

function childFrame() {
  const listeners = new Map<string, (event: Record<string, unknown>) => void>();
  const postMessage = vi.fn();
  let animation: (() => void) | undefined;
  runInNewContext(script, {
    window: {
      top: { postMessage },
      addEventListener: (name: string, handler: (event: Record<string, unknown>) => void) =>
        listeners.set(name, handler),
    },
    location: { origin: 'http://127.0.0.1:38473' },
    requestAnimationFrame: (callback: () => void) => {
      animation = callback;

      return 1;
    },
    cancelAnimationFrame: () => {
      animation = undefined;
    },
  });
  const capture = vi.fn();

  const emit = (name: string, values: Record<string, unknown> = {}) =>
    listeners.get(name)!({
      button: 2,
      buttons: 2,
      pointerId: 1,
      screenX: 100,
      screenY: 200,
      preventDefault: vi.fn(),
      stopImmediatePropagation: vi.fn(),
      target: { setPointerCapture: capture },
      ...values,
    });

  return { emit, capture, postMessage, animate: () => animation?.() };
}

describe('native overlay drag gestures', () => {
  it('leaves left clicks available to plugins', () => {
    const frame = childFrame();
    frame.emit('pointerdown', { button: 0 });
    frame.emit('pointermove');
    expect(frame.capture).not.toHaveBeenCalled();
    expect(frame.postMessage).not.toHaveBeenCalled();
  });

  it('coalesces movement and finishes with the release position', () => {
    const frame = childFrame();
    frame.emit('pointerdown');
    frame.emit('pointermove', { screenX: 120 });
    frame.emit('pointermove', { screenX: 150 });
    frame.animate();
    frame.emit('pointermove', { screenX: 170 });
    frame.emit('pointerup', { screenX: 180, screenY: 210 });
    frame.animate();
    expect(frame.capture).toHaveBeenCalledWith(1);
    expect(
      frame.postMessage.mock.calls.map(([message]) => [message.phase, message.x, message.y]),
    ).toEqual([
      ['start', 100, 200],
      ['move', 150, 200],
      ['end', 180, 210],
    ]);
  });

  it('ends once if pointer capture is lost after releasing', () => {
    const frame = childFrame();
    frame.emit('pointerdown');
    frame.emit('pointerup');
    frame.emit('lostpointercapture');
    expect(frame.postMessage).toHaveBeenCalledTimes(2);
  });
});
