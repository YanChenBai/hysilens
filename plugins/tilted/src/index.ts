import { definePlugin } from '@hysilens/plugin';
import type { ConfigOf, LyricDocument } from '@hysilens/plugin';

import type config from '../hysilens.config';

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

function secondary(lyrics: LyricDocument | null, kind: string, time: number) {
  return lyrics?.tracks
    .find(track => track.kind === kind)
    ?.lines.find(line => line.startMs <= time && (line.endMs === null || line.endMs > time))
    ?.text.trim();
}

function addText(parent: HTMLElement, text: string | undefined, className: string) {
  if (!text) {
    return;
  }

  const node = div(className);
  node.textContent = text;
  parent.append(node);
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
      fontFamily: `${JSON.stringify(theme.fontFamily)}, serif`,
      fontSize: `${theme.fontSize}px`,
      fontWeight: theme.fontWeight,
      color: theme.color,
    });

    const variables = {
      '--perspective': `${theme.perspective}px`,
      '--rotate-x': `${theme.rotateX}deg`,
      '--rotate-y': `${theme.rotateY}deg`,
      '--tilt': `${theme.tilt}deg`,
      '--skew': `${theme.skew}deg`,
      '--line-gap': `${theme.lineGap}px`,
      '--active-color': theme.activeColor,
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
      addText(row, line.text, 'original');

      if (state.theme.romanization) {
        addText(row, secondary(state.document, 'romanization', line.startMs), 'secondary');
      }

      if (state.theme.translation) {
        addText(row, secondary(state.document, 'translation', line.startMs), 'secondary');
      }

      return row;
    });
    track.replaceChildren(...rows);
  }

  function layout() {
    if (!current) {
      return;
    }

    const count = current.theme.visibleLines;
    const start = Math.max(0, focused - Math.floor(count / 2));
    const visible = rows.slice(start, start + count);
    const first = visible[0];
    const last = visible.at(-1);

    if (!first || !last) {
      return;
    }

    viewport.style.height = `${last.offsetTop + last.offsetHeight - first.offsetTop}px`;
    track.style.transform = `translateY(-${first.offsetTop}px)`;
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
    layout();
  }

  const observer = new ResizeObserver(layout);
  observer.observe(plane);
  document.fonts.addEventListener('loadingdone', layout);

  return {
    render,
    dispose() {
      observer.disconnect();
      document.fonts.removeEventListener('loadingdone', layout);
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
