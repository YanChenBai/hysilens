import { definePlugin } from '@hysilens/plugin';

// Deliberate value import: the build must reject it.
import config from '../hysilens.config';

export default definePlugin({
  setup({ root }) {
    root.textContent = config.name;
  },
});
