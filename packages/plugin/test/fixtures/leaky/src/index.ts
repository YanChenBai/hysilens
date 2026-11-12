import { definePlugin } from '@hysilens/plugin';

/* 故意使用值导入：构建必须拒绝 */
import config from '../hysilens.config';

export default definePlugin({
  setup({ root }) {
    root.textContent = config.name;
  },
});
