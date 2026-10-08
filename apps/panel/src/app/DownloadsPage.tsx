// zaminpanel://downloads/ — the founder's reserved future internal URL
// (§58), live now that a versioned channel exists to show (ADR-0029).
// The versioned pre-releases with their signed installers and portable
// archives, the installed build marked, the anchor's honest role stated.

import { useEffect, useState } from "react";
import { listPublishedReleases, type PublishedRelease } from "../state/releases";
import { useUpdates } from "../state/updates";
import { describeError, type DescribedError } from "../state/errors";
import { ErrorNote } from "../ui/ErrorNote";
import shared from "./internalPage.module.css";
import styles from "./DownloadsPage.module.css";

function formatWhen(iso: string | null): string {
  if (!iso) return "—";
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? "—" : date.toLocaleDateString();
}

function formatSize(bytes: number): string {
  if (bytes <= 0) return "";
  const mb = bytes / (1024 * 1024);
  return ` · ${mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`}`;
}

function AssetLink({ asset }: { asset: PublishedRelease["assets"][number] }) {
  const isInstaller =
    asset.browserDownloadUrl.endsWith("-setup.exe") ||
    asset.browserDownloadUrl.endsWith(".AppImage");
  return (
    <a
      className={`${shared.mono} ${styles.asset} ${isInstaller ? styles.installer : ""}`}
      href={asset.browserDownloadUrl}
      target="_blank"
      rel="noreferrer"
    >
      {asset.name}
      <span className={styles.assetSize}>{formatSize(asset.size)}</span>
    </a>
  );
}

function ReleaseRow({
  release,
  installed,
}: {
  release: PublishedRelease;
  installed: string | null;
}) {
  const version = release.tagName.replace(/^v/, "");
  const isInstalled = installed !== null && version === installed;
  return (
    <li className={shared.row}>
      <div className={shared.rowHead}>
        <span className={styles.version}>{release.tagName}</span>
        {isInstalled ? <span className={styles.installedChip}>Installed</span> : null}
        {release.prerelease ? <span className={styles.preChip}>Pre-release</span> : null}
        <span className={styles.when}>{formatWhen(release.publishedAt)}</span>
      </div>
      <p className={styles.releaseName}>{release.name}</p>
      {release.assets.length > 0 ? (
        <ul className={styles.assets} aria-label={`Downloads in ${release.tagName}`}>
          {release.assets.map((asset) => (
            <li key={asset.name}>
              <AssetLink asset={asset} />
            </li>
          ))}
        </ul>
      ) : (
        <p className={shared.note}>No downloadable assets in this entry.</p>
      )}
    </li>
  );
}

export function DownloadsPage() {
  const installedVersion = useUpdates((s) => s.installedVersion);
  const [releases, setReleases] = useState<PublishedRelease[] | null>(null);
  const [error, setError] = useState<DescribedError | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    listPublishedReleases()
      .then((answer) => {
        setReleases(answer);
        setError(null);
      })
      .catch((cause: unknown) => setError(describeError(cause)))
      .finally(() => setLoading(false));
  }, []);

  return (
    <div className={shared.page}>
      <header className={shared.head}>
        <h1 className={shared.title}>Downloads</h1>
        <p className={shared.subtitle}>
          The development channel ships versioned pre-releases; the panel updates itself from
          the same channel — the manifest anchor's URL never moves.
        </p>
      </header>

      {error ? <ErrorNote error={error} /> : null}
      {loading ? <p className={shared.note}>Asking the releases channel…</p> : null}

      {releases !== null && !loading ? (
        releases.length === 0 ? (
          <p className={shared.note}>The channel has no published releases yet.</p>
        ) : (
          <ul className={shared.list} aria-label="Published releases">
            {releases.map((release) => (
              <ReleaseRow
                key={release.tagName}
                release={release}
                installed={installedVersion}
              />
            ))}
          </ul>
        )
      ) : null}

      {releases !== null && !loading ? (
        <p className={shared.note}>
          Everything here is pre-release software until v1.0.0. The in-app updater verifies the
          minisign signature before anything is applied — manual downloads are for trying a
          specific version.
        </p>
      ) : null}
    </div>
  );
}
