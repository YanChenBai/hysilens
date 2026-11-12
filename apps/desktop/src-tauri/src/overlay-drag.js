/**
 * 注入到本地 overlay WebView，见 `local_overlay.rs`
 *
 * 顶层 frame 通过串行 promise 队列把 `size` 与 `drag` 消息转发给 Tauri bridge，
 * 因此窗口 resize 与 drag 不会交错执行；在 Windows 上 native watcher 会设置
 * `__HYSILENS_NATIVE_DRAG__`，从而禁用下面的 DOM fallback，因为透明 WebView
 * 无法可靠投递 pointer event；所有坐标都是 CSS screen pixel
 */
(() => {
  const kind = 'hysilens:local-overlay';

  if (window === window.top) {
    let queue = Promise.resolve();

    window.addEventListener('message', event => {
      if (event.origin !== location.origin || event.data?.kind !== kind) {
        return;
      }

      const { action, phase, x, y, width, height } = event.data;

      /* `size` 由本窗口自己发出，`drag` 消息来自 overlay 页面 */
      if (action === 'size' && event.source === window) {
        queue = queue
          .then(() => window.__TAURI_INTERNALS__.invoke('local_overlay_size', { width, height }))
          .catch(console.error);
      } else if (action === 'drag') {
        queue = queue
          .then(() => window.__TAURI_INTERNALS__.invoke('local_overlay_drag', { phase, x, y }))
          .catch(console.error);
      }
    });
    window.dispatchEvent(new Event('hysilens:overlay-ready'));
  }

  let pointer = null;
  let point = null;
  let frame = 0;

  const send = phase =>
    window.top.postMessage({ kind, action: 'drag', phase, ...point }, location.origin);

  /* 保证 drag 只结束一次，即使 pointerup 与 lostpointercapture 同时触发 */
  const finish = () => {
    if (pointer === null) {
      return;
    }

    cancelAnimationFrame(frame);
    frame = 0;
    pointer = null;
    send('end');
  };

  /* 右键就是 drag 手势，这里显式屏蔽 WebView context menu */
  window.addEventListener('contextmenu', event => event.preventDefault(), true);

  if (window.__HYSILENS_NATIVE_DRAG__) {
    return;
  }

  window.addEventListener(
    'pointerdown',
    event => {
      if (event.button !== 2 || pointer !== null) {
        return;
      }

      event.preventDefault();
      event.stopImmediatePropagation();
      pointer = event.pointerId;
      point = { x: event.screenX, y: event.screenY };
      event.target.setPointerCapture(pointer);
      send('start');
    },
    true,
  );
  window.addEventListener(
    'pointermove',
    event => {
      if (event.pointerId !== pointer) {
        return;
      }

      point = { x: event.screenX, y: event.screenY };

      if (!(event.buttons & 2)) {
        finish();

        return;
      }

      if (!frame) {
        /* 把多次 move 合并为每 frame 一条消息，窗口只需要最新的坐标 */
        frame = requestAnimationFrame(() => {
          frame = 0;
          send('move');
        });
      }
    },
    true,
  );
  window.addEventListener(
    'pointerup',
    event => {
      if (event.pointerId !== pointer || event.button !== 2) {
        return;
      }

      point = { x: event.screenX, y: event.screenY };
      finish();
    },
    true,
  );
  window.addEventListener('pointercancel', finish, true);
  window.addEventListener('lostpointercapture', finish, true);
})();
