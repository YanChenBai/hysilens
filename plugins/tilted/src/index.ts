import { definePlugin } from '@hysilens/plugin';
import type { ConfigOf, LyricDocument } from '@hysilens/plugin';

import type config from '../hysilens.config';
import { centeredWindow } from './layout';
import { preset } from './preset';

import './style.css';

type Theme = ConfigOf<typeof config>;

interface State {
  theme: Theme;
  document: LyricDocument | null;
  /** Lyric time with offset applied. */
  time: number | null;
  visible: boolean;
}

function div(className: string) {
  const node = document.createElement('div');
  node.className = className;

  return node;
}

function textShadow(theme: Theme): string {
  const shadowAlpha = Math.round(theme.shadowOpacity * 255)
    .toString(16)
    .padStart(2, '0');

  const shadows = Array.from({ length: Math.ceil(theme.depth * 2) }, (_, index) => {
    const offset = Math.min(theme.depth, (index + 1) / 2);

    return `${offset}px ${offset}px 0 ${theme.depthColor}`;
  });

  if (theme.shadow) {
    shadows.push(
      `${theme.shadowX}px ${theme.shadowY}px ${theme.shadowBlur}px ${theme.shadowColor}${shadowAlpha}`,
    );
  }

  return shadows.join(', ') || 'none';
}

function lyricFace(text: string) {
  const original = div('original');
  original.textContent = text;

  return original;
}

/** DOM-measured layout: rebuilt on theme or document changes, re-focused on line changes. */
function createStage(container: HTMLElement) {
  const stage = document.createElement('main');
  stage.className = 'tilted';
  const plane = div('plane');
  const viewport = div('viewport');
  const track = div('track');
  viewport.append(track);
  plane.append(viewport);
  stage.append(plane);
  container.append(stage);

  let current: State | null = null;
  let previousDocument: LyricDocument | null = null;
  let previousTheme: Theme | undefined;
  let focused = -1;
  let rows: HTMLElement[] = [];

  function applyTheme(theme: Theme) {
    Object.assign(stage.style, {
      fontFamily: `${JSON.stringify(theme.fontFamily)}, 'ZCOOLXiaoWei', serif`,
      fontSize: `${theme.fontSize}px`,
      fontWeight: String(theme.fontWeight),
      color: theme.color,
    });

    const variables = {
      '--perspective': `${preset.perspective}px`,
      '--rotate-y': `${preset.rotateY}deg`,
      '--line-gap': `${theme.lineGap}px`,
      '--active-color': theme.activeColor,
      '--active-scale': String(preset.activeScale),
      '--inactive-opacity': String(theme.inactiveOpacity),
      '--stroke-width': `${theme.strokeWidth}px`,
      '--stroke-color': theme.strokeColor,
      '--text-shadow': textShadow(theme),
    };

    for (const [name, value] of Object.entries(variables)) {
      stage.style.setProperty(name, value);
    }
  }

  function rebuild(state: State) {
    applyTheme(state.theme);
    const lines = state.document?.tracks.find(track => track.kind === 'original')?.lines ?? [];
    rows = lines.map(line => {
      const row = div('line');
      row.append(lyricFace(line.text));

      return row;
    });
    track.replaceChildren(...rows);
  }

  function layout(snap = false) {
    if (!current || stage.hidden || plane.clientWidth === 0) {
      return;
    }

    const window = centeredWindow(
      rows.map(row => ({ top: row.offsetTop, height: row.offsetHeight })),
      focused,
      current.theme.visibleLines,
      preset.activeScale,
    );

    if (!window) {
      viewport.style.height = '0px';

      return;
    }

    rows.forEach((row, index) => {
      row.classList.toggle('in-window', index >= window.start && index < window.end);
    });

    if (snap) {
      track.style.transition = 'none';
    }

    viewport.style.height = `${window.height}px`;
    track.style.transform = `translateY(${window.translateY}px)`;

    // Fit the whole projected lyric plane inside the available canvas.
    plane.style.setProperty('--fit', '1');
    const bounds = plane.getBoundingClientRect();

    const fit = Math.min(
      1,
      (stage.clientWidth - 48) / bounds.width,
      (stage.clientHeight - 48) / bounds.height,
    );

    plane.style.setProperty('--fit', String(Math.max(0.01, fit)));

    // bounds above flushes the new track position before transitions are restored.
    if (snap) {
      track.style.removeProperty('transition');
    }
  }

  function render(state: State) {
    current = state;
    stage.hidden = !state.visible || state.time === null || state.document === null;

    if (stage.hidden || state.time === null) {
      return;
    }

    // The runtime hands out a new config object only when it changes, so identity suffices.
    const changed = previousDocument !== state.document || previousTheme !== state.theme;

    if (changed) {
      previousDocument = state.document;
      previousTheme = state.theme;
      rebuild(state);
    }

    const time = state.time;
    const lines = state.document?.tracks.find(track => track.kind === 'original')?.lines ?? [];
    const nextFocus = lines.findLastIndex(line => line.startMs <= time);

    if (!changed && nextFocus === focused) {
      return;
    }

    focused = nextFocus;
    rows.forEach((row, index) => {
      row.classList.toggle('current', index === focused);
      row.setAttribute('aria-current', String(index === focused));
    });
    layout(changed);
  }

  const relayout = () => layout(true);
  const observer = new ResizeObserver(relayout);
  observer.observe(plane);
  observer.observe(stage);
  document.fonts.addEventListener('loadingdone', relayout);

  return {
    render,
    dispose() {
      observer.disconnect();
      document.fonts.removeEventListener('loadingdone', relayout);
      stage.remove();
    },
  };
}

export default definePlugin<typeof config>({
  setup({ root, config: readConfig, lyrics, playback, time, onMounted, onUnmounted }) {
    let stage: ReturnType<typeof createStage> | undefined;
    let frame = 0;

    const tick = () => {
      stage?.render({
        theme: readConfig(),
        document: lyrics(),
        time: time(),
        visible: playback().visible,
      });
      frame = requestAnimationFrame(tick);
    };

    onMounted(() => {
      stage = createStage(root);
      tick();
    });

    onUnmounted(() => {
      cancelAnimationFrame(frame);
      stage?.dispose();
      stage = undefined;
    });
  },
});
