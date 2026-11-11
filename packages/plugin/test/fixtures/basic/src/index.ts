import { definePlugin } from '@hysilens/plugin';

import type config from '../hysilens.config';

import './style.css';

export default definePlugin<typeof config>({
  setup({ root, config, onMounted }) {
    onMounted(() => {
      root.dataset.env = process.env.NODE_ENV;
      root.textContent = `${config().size}px ${config().fontFamily}`;
    });
  },
});
