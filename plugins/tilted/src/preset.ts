/**
 * 固定几何参数；字体、颜色和显示行数单独配置
 */
export const preset = {
  /**
   * CSS perspective 距离，单位像素；值越大投影越平
   */
  perspective: 1000,

  /**
   * 绕 Y 轴的旋转角度；负值让平面向右倾斜
   */
  rotateY: -40,

  /**
   * 基准字号，单位像素
   */
  fontSize: 42,
  fontWeight: '400',

  /**
   * 歌词行之间的垂直间距，单位像素
   */
  lineGap: 32,

  /**
   * 当前行应用的缩放
   */
  activeScale: 1.4,

  /**
   * 非当前行透明度，取值 0–1
   */
  inactiveOpacity: 0.52,

  /**
   * 歌词描边宽度，单位像素
   */
  strokeWidth: 1.2,

  /**
   * 字形后面的立体层数，单位像素
   */
  depth: 1,

  shadow: true,

  /**
   * 投影透明度和模糊半径，模糊单位像素
   */
  shadowOpacity: 0.75,
  shadowBlur: 4,

  /**
   * 投影偏移，单位像素
   */
  shadowX: 1,
  shadowY: 3,
} as const;
