// Level meter canvas renderer — stateless ES module.
// Called by plugin-loader.ts on every requestAnimationFrame.
// data: ArrayBuffer (16 bytes) = [left_peak, right_peak, left_hold, right_hold] as f32 LE.

const GREEN_BOUNDARY  = -18; // dBFS
const YELLOW_BOUNDARY = -6;  // dBFS
const DB_FLOOR        = -60; // dBFS (minimum displayed)

/** Convert linear amplitude [0, 1] to dBFS clamped to [DB_FLOOR, 0]. */
function toDb(linear) {
  return Math.max(DB_FLOOR, Math.min(0, 20 * Math.log10(Math.max(linear, 1e-6))));
}

/** Map dBFS value to canvas x-coordinate (0 dBFS = full width). */
function dbToX(db, width) {
  return (db - DB_FLOOR) / (0 - DB_FLOOR) * width;
}

export default {
  /** No persistent state needed. */
  async init(_canvasElements) {},

  /**
   * @param {ArrayBuffer} data   16-byte snapshot from render_snapshot()
   * @param {string}      canvasId  "level_left" or "level_right"
   * @param {HTMLCanvasElement} canvas
   */
  render(data, canvasId, canvas) {
    // Keep canvas resolution in sync with its CSS size.
    if (canvas.width !== canvas.offsetWidth)   canvas.width  = canvas.offsetWidth;
    if (canvas.height !== canvas.offsetHeight) canvas.height = canvas.offsetHeight;

    const values = new Float32Array(data);
    const [leftPeak, rightPeak, leftHold, rightHold] = values;

    let peak, hold;
    if (canvasId === 'level_left') {
      peak = leftPeak;
      hold = leftHold;
    } else {
      peak = rightPeak;
      hold = rightHold;
    }

    const ctx = canvas.getContext('2d');
    const w   = canvas.width;
    const h   = canvas.height;

    // dBFS boundaries as x-coordinates.
    const xGreenEnd  = dbToX(GREEN_BOUNDARY,  w);
    const xYellowEnd = dbToX(YELLOW_BOUNDARY, w);
    const peakX      = dbToX(toDb(peak),      w);
    const holdX      = dbToX(toDb(hold),      w);

    // Clear
    ctx.clearRect(0,0,w,h);

    // Draw peak bar in three colour zones, each clipped to peakX.
    const zones = [
      { color: '#4caf50', x0: 0,          x1: xGreenEnd  },
      { color: '#ffeb3b', x0: xGreenEnd,  x1: xYellowEnd },
      { color: '#f44336', x0: xYellowEnd, x1: w          },
    ];

    for (const { color, x0, x1 } of zones) {
      const zoneRight = Math.min(x1, peakX);
      if (zoneRight <= x0) continue;
      ctx.fillStyle = color;
      ctx.fillRect(x0, 0, zoneRight - x0, h);
    }

    // Draw peak-hold tick: 2px white vertical line.
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(holdX - 1, 0, 5, h);
  },
};
