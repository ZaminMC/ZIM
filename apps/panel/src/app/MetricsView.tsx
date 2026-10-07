// The Metrics tab: current numbers up top, two sparklines (CPU, memory)
// drawn as plain SVG from the bounded sample window. The subscription is
// owned by the server workspace (state/metrics.ts), so this view only
// reads the store — opening the tab never delays on a fresh subscribe.

import { useServerMetrics, formatBytes, formatCpu, formatUptime } from "../state/metrics";
import styles from "./MetricsView.module.css";

const VIEW_W = 600;
const VIEW_H = 120;
const PAD = 6;

function Sparkline({
  values,
  className,
  areaClassName,
  ceiling,
}: {
  values: number[];
  className?: string;
  areaClassName?: string;
  /** Fixed top of the scale (e.g. 100 for CPU%); absent = fit to data. */
  ceiling?: number;
}) {
  if (values.length < 2) return null;
  const peak =
    ceiling ?? Math.max(...values) * 1.15; // headroom, never a fake ceiling
  const floor = ceiling === undefined ? Math.min(...values) * 0.9 : 0;
  const span = Math.max(peak - floor, Number.EPSILON);
  const step = VIEW_W / (values.length - 1);
  const points = values.map((value, index) => {
    const x = index * step;
    const clamped = Math.min(Math.max(value, floor), peak);
    const y = VIEW_H - PAD - ((clamped - floor) / span) * (VIEW_H - PAD * 2);
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  });
  return (
    <svg
      className={styles.spark}
      viewBox={`0 0 ${VIEW_W} ${VIEW_H}`}
      preserveAspectRatio="none"
      role="img"
      aria-hidden="true"
    >
      <polygon
        className={areaClassName}
        points={`0,${VIEW_H} ${points.join(" ")} ${VIEW_W},${VIEW_H}`}
      />
      <polyline
        className={className}
        points={points.join(" ")}
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

export function MetricsView({ serverId }: { serverId: string }) {
  const samples = useServerMetrics(serverId);
  const latest = samples.at(-1);

  if (!latest) {
    return (
      <div className={styles.wrap}>
        <p className={styles.waiting}>
          No samples yet. The sampler publishes one per second while the
          server process is live.
        </p>
      </div>
    );
  }

  const cpuSeries = samples
    .map((sample) => sample.cpuPercent)
    .filter((value): value is number => value !== undefined);
  const rssSeries = samples
    .map((sample) => sample.rssBytes)
    .filter((value): value is number => value !== undefined);
  // One nonzero CPU reading makes the 0-line honest; a purely idle JVM
  // would otherwise collapse the scale onto its own noise.
  const cpuPeak = Math.max(100, ...cpuSeries);

  return (
    <div className={styles.wrap}>
      <div className={styles.stats}>
        <div className={styles.stat}>
          <span className={styles.statLabel}>CPU</span>
          <span className={styles.statValue}>{formatCpu(latest.cpuPercent)}</span>
        </div>
        <div className={styles.stat}>
          <span className={styles.statLabel}>Memory</span>
          <span className={styles.statValue}>{formatBytes(latest.rssBytes)}</span>
        </div>
        <div className={styles.stat}>
          <span className={styles.statLabel}>Players</span>
          <span className={styles.statValue}>
            {latest.players === undefined ? "—" : String(latest.players)}
          </span>
        </div>
        <div className={styles.stat}>
          <span className={styles.statLabel}>Uptime</span>
          <span className={styles.statValue}>{formatUptime(latest.uptimeMs)}</span>
        </div>
      </div>

      <div className={styles.chartCard}>
        <div className={styles.chartHead}>
          <span>CPU</span>
          <span className={styles.chartNow}>{formatCpu(latest.cpuPercent)}</span>
        </div>
        {cpuSeries.length >= 2 ? (
          <Sparkline
            values={cpuSeries}
            className={styles.cpuLine}
            areaClassName={styles.cpuArea}
            ceiling={cpuPeak}
          />
        ) : (
          <p className={styles.waiting}>collecting samples…</p>
        )}
      </div>

      <div className={styles.chartCard}>
        <div className={styles.chartHead}>
          <span>Memory (RSS)</span>
          <span className={styles.chartNow}>{formatBytes(latest.rssBytes)}</span>
        </div>
        {rssSeries.length >= 2 ? (
          <Sparkline
            values={rssSeries}
            className={styles.rssLine}
            areaClassName={styles.rssArea}
          />
        ) : (
          <p className={styles.waiting}>collecting samples…</p>
        )}
      </div>

      <p className={styles.note}>
        One sample per second, {samples.length} in view. TPS appears only when a
        server actually reports it — the panel never guesses.
      </p>
    </div>
  );
}
