export default {
  async init(canvasElements) {
    canvasElements.forEach((c) => {
      this.render([], c.canvas, c.canvasId);
    });
  },

  render(data, canvasId, canvas) {
    if (canvas.width !== canvas.offsetWidth)  canvas.width  = canvas.offsetWidth;
    if (canvas.height !== canvas.offsetHeight) canvas.height = canvas.offsetHeight;

    const samples = new Float32Array(data);
    if (samples.length === 0) return;

    const ctx = canvas.getContext('2d');
    const w = canvas.width;
    const h = canvas.height;
    const mid = h / 2;
    const step = w / samples.length;

    ctx.clearRect(0, 0, w, h);
   
    ctx.strokeStyle = '#00ff00';
    ctx.beginPath();
    ctx.moveTo(0, mid + samples[0] * mid);
    for (let i = 1; i < samples.length; i++) {
      ctx.lineTo(i * step, mid + samples[i] * mid);
    }
    ctx.stroke();
  }
};
