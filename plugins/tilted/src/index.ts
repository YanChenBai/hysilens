import { definePlugin } from '@hysilens/plugin';
import type { ConfigOf, LyricDocument } from '@hysilens/plugin';

import type config from '../hysilens.config';
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
  const shadowAlpha = Math.round(preset.shadowOpacity * 255)
    .toString(16)
    .padStart(2, '0');

  const shadows = Array.from({ length: Math.ceil(preset.depth * 2) }, (_, index) => {
    const offset = Math.min(preset.depth, (index + 1) / 2);

    return `${offset}px ${offset}px 0 ${theme.depthColor}`;
  });

  if (preset.shadow) {
    shadows.push(
      `${preset.shadowX}px ${preset.shadowY}px ${preset.shadowBlur}px ${theme.shadowColor}${shadowAlpha}`,
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
      fontSize: `${preset.fontSize}px`,
      fontWeight: preset.fontWeight,
      color: theme.color,
    });

    const variables = {
      '--perspective': `${preset.perspective}px`,
      '--rotate-y': `${preset.rotateY}deg`,
      '--line-gap': `${preset.lineGap}px`,
      '--active-color': theme.activeColor,
      '--active-scale': String(preset.activeScale),
      '--inactive-opacity': String(preset.inactiveOpacity),
      '--stroke-width': `${preset.strokeWidth}px`,
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

  function layout() {
    if (!current || stage.hidden || plane.clientWidth === 0) {
      return;
    }

    const count = current.theme.visibleLines;
    const start = Math.max(0, focused - Math.floor(count / 2));
    const visible = rows.slice(start, start + count);
    rows.forEach((row, index) => {
      row.classList.toggle('in-window', index >= start && index < start + count);
    });
    const first = visible[0];
    const last = visible.at(-1);

    if (!first || !last) {
      return;
    }

    // Include the scaled row's visual bounds, especially when only one row is displayed.
    const active = rows[focused];
    const expansion = active ? (active.offsetHeight * (preset.activeScale - 1)) / 2 : 0;
    const top = active ? Math.min(first.offsetTop, active.offsetTop - expansion) : first.offsetTop;

    const bottom = Math.max(
      last.offsetTop + last.offsetHeight,
      active ? active.offsetTop + active.offsetHeight + expansion : 0,
    );

    viewport.style.height = `${bottom - top}px`;
    track.style.transform = `translateY(${-top}px)`;

    // Fit the whole projected lyric plane inside the available canvas.
    plane.style.setProperty('--fit', '1');
    const bounds = plane.getBoundingClientRect();

    const fit = Math.min(
      1,
      (stage.clientWidth - 48) / bounds.width,
      (stage.clientHeight - 48) / bounds.height,
    );

    plane.style.setProperty('--fit', String(Math.max(0.01, fit)));
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
