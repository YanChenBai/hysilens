import { isConnectMessage, WAITING_KIND } from './protocol';
import { isPlugin, runPlugin } from './runtime';

/**
 * Entry injected by the build around the plugin's default export. Waits for the host's port,
 * then drives the plugin. Announces itself so a host that missed `load` can still connect.
 */
export function start(plugin: unknown): void {
  if (!isPlugin(plugin)) {
    throw new TypeError('插件入口必须 export default definePlugin(...)');
  }

  const definition = plugin;
  const root = document.createElement('div');
  root.dataset.hysilensRoot = '';
  root.style.cssText = 'position:fixed;inset:0;';
  document.body.append(root);

  function connect(event: MessageEvent) {
    const port = event.ports[0];

    if (event.source !== window.parent || !isConnectMessage(event.data) || !port) {
      return;
    }

    window.removeEventListener('message', connect);

    const runtime = runPlugin(definition, {
      port,
      root,
      baseUrl: document.baseURI,
      now: () => performance.now(),
    });

    window.addEventListener('error', event => runtime.report(event.error ?? event.message));
    window.addEventListener('unhandledrejection', event => runtime.report(event.reason));
  }

  window.addEventListener('message', connect);
  window.parent.postMessage({ kind: WAITING_KIND }, '*');
}
