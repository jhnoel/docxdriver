// Summary statistics over an array of millisecond samples.

export function summarize(samples) {
  const sorted = [...samples].sort((a, b) => a - b);
  const n = sorted.length;
  const sum = sorted.reduce((a, b) => a + b, 0);
  return {
    n,
    min: sorted[0],
    max: sorted[n - 1],
    mean: sum / n,
    median: n % 2 === 1 ? sorted[(n - 1) / 2] : (sorted[n / 2 - 1] + sorted[n / 2]) / 2,
    p95: sorted[Math.min(n - 1, Math.max(0, Math.ceil(n * 0.95) - 1))],
    p99: sorted[Math.min(n - 1, Math.max(0, Math.ceil(n * 0.99) - 1))],
  };
}

// Per-second throughput from per-op ms samples.
export function opsPerSecond(samples) {
  const mean = samples.reduce((a, b) => a + b, 0) / samples.length;
  return mean === 0 ? Infinity : 1000 / mean;
}

export function formatMs(ms) {
  if (ms >= 1000) return `${(ms / 1000).toFixed(2)}s`;
  if (ms >= 1) return `${ms.toFixed(2)}ms`;
  return `${(ms * 1000).toFixed(1)}µs`;
}
