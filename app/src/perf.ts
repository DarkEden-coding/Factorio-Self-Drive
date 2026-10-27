const WINDOW_MS = 500;

// Rolling frame statistics: fps from frame spacing, frame ms from the JS time spent in a frame
// (GPU raster time is not included).
export class FrameStats {
  fps = 0;
  avgMs = 0;
  maxMs = 0;
  private frames = 0;
  private workSum = 0;
  private workMax = 0;
  private windowStart = performance.now();

  record(now: number, workMs: number): void {
    this.frames++;
    this.workSum += workMs;
    this.workMax = Math.max(this.workMax, workMs);
    const elapsed = now - this.windowStart;
    if (elapsed < WINDOW_MS) return;
    this.fps = (this.frames * 1000) / elapsed;
    this.avgMs = this.workSum / this.frames;
    this.maxMs = this.workMax;
    this.frames = 0;
    this.workSum = 0;
    this.workMax = 0;
    this.windowStart = now;
  }

  label(): string {
    return `${this.fps.toFixed(0)} fps  ${this.avgMs.toFixed(1)} ms (max ${this.maxMs.toFixed(1)})`;
  }
}
