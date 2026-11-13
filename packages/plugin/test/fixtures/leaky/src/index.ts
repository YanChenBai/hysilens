import { definePlugin } from '@hysilens/plugin';

import config from '../hysilens.config';

export default definePlugin<typeof config>({
  setup({ root }) {
    root.textContent = JSON.stringify(config);
  },
});
