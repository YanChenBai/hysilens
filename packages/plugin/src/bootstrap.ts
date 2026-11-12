import { isConnectMessage, WAITING_KIND } from './protocol';
import { isPlugin, runPlugin } from './runtime';

/**
 * 由构建过程注入到 plugin 默认导出外层的入口
 * 等待 host 的 port 后驱动 plugin；同时主动广播自身，让错过 `load` 的 host 仍能连接
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
