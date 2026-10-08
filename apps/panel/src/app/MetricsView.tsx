// The Metrics tab (founder §31): the current/allowed rows up top — CPU
// against the host's cores, memory against the JVM's -Xmx, players
// against the server's own max-players — then the sparklines drawn as
// plain SVG from the bounded sample window. The subscription is owned by
// the server workspace (state/metrics.ts), so this view only reads the
// store — opening the tab never delays on a fresh subscribe. Every
// allowance is a real number from the thing that owns it (the webview's
// hardwareConcurrency, the config model, the Server List Ping); none is
// invented, and an unknown ceiling renders without one rather than
// pretending.

import { useEffect, useState } from "react";
import { useServerMetrics, formatBytes, formatCpu, formatUptime } from "../state/metrics";
import { getServerConfig, listPlayers } from "../state/actions";
import styles from "./MetricsView.module.css";

/** How often the players' ceiling re-pings: Server List Ping answers the
 *  server itself, and the server owns the number. */
const PLAYERS_ALLOWED_REFRESH_MS = 15_000;

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

interface Allowances {
  /** The JVM heap ceiling from the layered config model (ADR-0007). */
  ramMb?: number;
  /** The server's own max-players, from Server List Ping. */
  players?: number;
  /** The webview host's logical cores × 100% — the process ceiling. */
  cpuPercent?: number;
}

function Meter({
  label,
  currentText,
  allowedText,
  fraction,
  barClass,
}: {
  label: string;
  currentText: string;
  allowedText?: string;
  /** 0..1 of the allowance; absent when no ceiling is known. */
  fraction?: number;
  barClass?: string;
}) {
  return (
    <div className={styles.meter}>
      <div className={styles.meterHead}>
        <span className={styles.statLabel}>{label}</span>
        <span className={styles.statValue}>
          {currentText}
          {allowedText ? <span className={styles.allowed}> / {allowedText}</span> : null}
        </span>
      </div>
      {fraction !== undefined && Number.isFinite(fraction) ? (
        <div
          className={styles.meterTrack}
          role="meter"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(Math.min(1, Math.max(0, fraction)) * 100)}
          aria-label={`${label} usage`}
        >
          <div
            className={`${styles.meterFill} ${barClass ?? ""}`}
            style={{ width: `${Math.min(100, Math.max(0, fraction * 100))}%` }}
          />
        </div>
      ) : null}
    </div>
  );
}

export function MetricsView({ serverId }: { serverId: string }) {
  const samples = useServerMetrics(serverId);
  const latest = samples.at(-1);
  const [allowance, setAllowance] = useState<Allowances>({});

  // The allowances come from their owners: the config model owns the
  // heap, the webview host owns the core count, the server's ping owns
  // max-players. Each arrives when it arrives; none blocks the rows.
  useEffect(() => {
    let alive = true;
    void getServerConfig(serverId)
      .then((config) => {
        if (alive) setAllowance((prev) => ({ ...prev, ramMb: config.effective.maxMemoryMb }));
      })
      .catch(() => {}); // the wire answers again when the tab re-opens
    const cores =
      typeof navigator !== "undefined" && navigator.hardwareConcurrency > 0
        ? navigator.hardwareConcurrency
        : undefined;
    if (cores !== undefined) {
      setAllowance((prev) => ({ ...prev, cpuPercent: cores * 100 }));
    }
    return () => {
      alive = false;
    };
  }, [serverId]);

  useEffect(() => {
    let alive = true;
    let timer = 0;
    const ask = () => {
      void listPlayers(serverId)
        .then((answer) => {
          if (alive && answer.max !== undefined) {
            setAllowance((prev) => ({ ...prev, players: answer.max }));
          }
        })
        .catch(() => {}); // an offline server is a normal state here
      timer = window.setTimeout(ask, PLAYERS_ALLOWED_REFRESH_MS);
    };
    ask();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [serverId]);

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
        <Meter
          label="CPU"
          currentText={formatCpu(latest.cpuPercent)}
          allowedText={
            allowance.cpuPercent === undefined ? undefined : formatCpu(allowance.cpuPercent)
          }
          fraction={
            latest.cpuPercent !== undefined && allowance.cpuPercent !== undefined
              ? latest.cpuPercent / allowance.cpuPercent
              : undefined
          }
          barClass={styles.cpuBar}
        />
        <Meter
          label="Memory"
          currentText={formatBytes(latest.rssBytes)}
          allowedText={allowance.ramMb === undefined ? undefined : formatBytes(allowance.ramMb * 1024 * 1024)}
          fraction={
            latest.rssBytes !== undefined && allowance.ramMb !== undefined
              ? latest.rssBytes / (allowance.ramMb * 1024 * 1024)
              : undefined
          }
          barClass={styles.rssBar}
        />
        <Meter
          label="Players"
          currentText={latest.players === undefined ? "—" : String(latest.players)}
          allowedText={allowance.players === undefined ? undefined : String(allowance.players)}
          fraction={
            latest.players !== undefined && allowance.players !== undefined && allowance.players > 0
              ? latest.players / allowance.players
              : undefined
          }
          barClass={styles.playersBar}
        />
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
