(() => {
  const kind = 'hysilens:local-overlay';

  if (window === window.top) {
    let queue = Promise.resolve();
    window.addEventListener('message', event => {
      if (event.origin !== location.origin || event.data?.kind !== kind) {
        return;
      }

      const { action, phase, x, y, width, height } = event.data;

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

  const finish = () => {
    if (pointer === null) {
      return;
    }

    cancelAnimationFrame(frame);
    frame = 0;
    pointer = null;
    send('end');
  };

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
