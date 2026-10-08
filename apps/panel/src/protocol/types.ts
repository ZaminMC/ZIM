// Mirror of the Zamin Protocol v1 wire shapes (zamin-protocol, Rust).
// Protocol spec §2 (handshake), §5 (servers), §6 (streams), §9 (logs).
// Field names are exactly what serde emits (camelCase / kebab-case tags);
// a mismatch here is a wire bug, not a refactor.

export const PROTOCOL_VERSION = 1;

// --- handshake (§2) ---

export interface ClientInfo {
  name: string;
  version: string;
}

export interface HelloParams {
  protocol: number;
  /** Remote transports require the token (ADR-0011); local transports
   *  ignore it — the OS user + socket ACL is the credential there. */
  auth?: string;
  client: ClientInfo;
}

export interface HelloResult {
  protocol: number;
  protocolMin: number;
  protocolMax: number;
  daemon: { name: string; version: string };
  capabilities: string[];
}

// --- servers (§5) ---

export type ServerState =
  | "not-running"
  | "starting"
  | "running"
  | "stopping"
  | "stopped"
  | "failed-preflight"
  | "crashed"
  | "adopting"
  | "unknown";

export type CrashPhase = "startup" | "runtime" | "shutdown";

export interface CrashClassification {
  phase: CrashPhase;
  exitCode?: number;
  evidence?: string;
}

export interface ServerSummary {
  serverId: string;
  displayName: string;
  state: ServerState;
}

export interface ServerDetails {
  serverId: string;
  displayName: string;
  state: ServerState;
  software?: string;
  version?: string;
  port?: number;
}

export interface ServerListResult {
  servers: ServerSummary[];
}

export interface RegisterServerParams {
  requestId: string;
  serverId: string;
  displayName: string;
  rootPath: string;
}

export interface RegisterServerResult {
  server: ServerDetails;
}

export interface RemoveServerParams {
  requestId: string;
  serverId: string;
}

export interface UpdateServerParams {
  requestId: string;
  serverId: string;
  displayName?: string;
}

export interface LifecycleResult {
  serverId: string;
  state: ServerState;
}

export interface StdinParams {
  requestId: string;
  serverId: string;
  line: string;
}

// --- streams (§6, ADR-0006) ---

export type StreamKind = "events" | "logs" | "metrics";

export type StreamCursor = { seq: number } | { file: string; offset: number };

export interface EventsSnapshot {
  servers: ServerSummary[];
}

export interface SubscribeResult {
  subscriptionId: string;
  cursor?: StreamCursor;
  snapshot?: EventsSnapshot;
  cursorInvalid?: boolean;
}

export type LogLevel = "info" | "warn" | "error" | "debug" | "unknown";

export interface LogLine {
  tsMs: number;
  level: LogLevel;
  thread?: string;
  line: string;
}

export interface MetricsSample {
  tsMs: number;
  cpuPercent?: number;
  rssBytes?: number;
  players?: number;
  tps?: number;
  uptimeMs?: number;
}

// --- jobs & backups (§7) ---

export type JobKind =
  | "server.create"
  | "backup.create"
  | "backup.restore"
  | "archive.extract"
  | "java.install"
  | "plugins.install"
  | "publish.execute";
export type JobState = "queued" | "running" | "succeeded" | "failed" | "cancelled";
export type JobOutcome = "succeeded" | "failed" | "cancelled";
export type BackupTaken = "live" | "cold";

export interface JobProgress {
  current: number;
  total?: number;
  unit?: string;
  message?: string;
}

export interface Job {
  jobId: string;
  kind: JobKind;
  serverId?: string;
  state: JobState;
  progress?: JobProgress;
  error?: ProtocolErrorObject;
  createdAtMs: number;
  startedAtMs?: number;
  endedAtMs?: number;
}

export interface BackupInfo {
  backupId: string;
  createdAtMs: number;
  sizeBytes: number;
  totalBytes: number;
  fileCount: number;
  label?: string;
  taken: BackupTaken;
}

export interface BackupsListResult {
  backups: BackupInfo[];
}

export interface BackupJobResult {
  kind: JobKind;
  job: Job;
}

export interface JobsListResult {
  jobs: Job[];
}

export interface CancelJobParams {
  requestId: string;
  jobId: string;
}

export type CoreEvent =
  | {
      type: "serverStateChanged";
      serverId: string;
      from: ServerState;
      to: ServerState;
      reason?: string;
      exitCode?: number;
      error?: ProtocolErrorObject;
      crash?: CrashClassification;
    }
  | { type: "jobStarted"; job: Job }
  | { type: "jobProgress"; jobId: string; progress: JobProgress }
  | {
      type: "jobCompleted";
      jobId: string;
      outcome: JobOutcome;
      error?: ProtocolErrorObject;
    };

export type StreamPayload =
  | { kind: "logs"; batch: LogLine[] }
  | { kind: "event"; event: CoreEvent }
  | { kind: "metrics"; sample: MetricsSample }
  | { kind: "missed"; missed: number };

export interface StreamNotification {
  stream: StreamKind;
  serverId?: string;
  seq: number;
  payload: StreamPayload;
}

export interface SubscribeParams {
  stream: StreamKind;
  serverId?: string;
  cursor?: StreamCursor;
}

// --- logs (§9, ADR-0006 file-backed catch-up) ---

export interface LogRangeParams {
  serverId: string;
  maxLines?: number;
  /** Byte-offset cursor: return lines ending at or before this offset
   *  (the previous page's startOffset). Absent — read from the tail. */
  beforeOffset?: number;
}

export interface LogRangeResult {
  file: string;
  lines: LogLine[];
  olderAvailable: boolean;
  /** Byte offset where the first returned line starts — a line boundary.
   *  Pass it as beforeOffset to page further back; 0 = nothing older. */
  startOffset: number;
}

// --- metrics (§6, ADR-0006 ring-backed history) ---

export interface MetricsRangeParams {
  serverId: string;
  /** Samples to return, newest kept. Defaults to 120; capped at 600. */
  maxSamples?: number;
}

export interface MetricsRangeResult {
  /** Chronological (oldest first). The ring is the entire stored
   *  history — bounded by design, no paging. */
  samples: MetricsSample[];
}

// --- files (§8, ADR-0009 rooted filesystem) ---

export type EntryKind = "file" | "directory";

export interface FilesEntry {
  name: string;
  kind: EntryKind;
  sizeBytes?: number;
  modifiedMs?: number;
  symlinkOutside?: boolean;
}

export interface FilesListParams {
  serverId: string;
  path: string;
  offset?: number;
  limit?: number;
}

export interface FilesListResult {
  path: string;
  entries: FilesEntry[];
  total: number;
}

// --- files.copy / files.search (the files slice, ADR-0021) ---

export interface FilesCopyParams {
  serverId: string;
  /** Root-relative source: a file or a whole folder. */
  from: string;
  /** Root-relative target. Must not exist — copies never overwrite. */
  to: string;
}

export interface FilesCopyResult {
  path: string;
  files: number;
  bytes: number;
}

export interface FilesSearchParams {
  serverId: string;
  /** Case-insensitive name substring; empty is a protocol error. */
  query: string;
  limit?: number;
}

export interface FilesSearchHit {
  path: string;
  kind: EntryKind;
  sizeBytes?: number;
  modifiedMs?: number;
}

export interface FilesSearchResult {
  hits: FilesSearchHit[];
  truncated: boolean;
  scanned: number;
}

export interface FilesReadParams {
  serverId: string;
  path: string;
  offset: number;
  maxBytes: number;
}

export interface FilesReadResult {
  data: string;
  eof: boolean;
  totalBytes: number;
}

export interface FilesWriteParams {
  serverId: string;
  stagingId?: string;
  content: string;
}

export interface FilesWriteResult {
  stagingId: string;
  bytesStaged: number;
}

export interface FilesCommitParams {
  serverId: string;
  stagingId: string;
  target: string;
}

export interface FilesCommitResult {
  path: string;
  sizeBytes: number;
}

// --- players (§5, Server List Ping) ---

export interface PlayerSample {
  name: string;
  id?: string;
}

export interface PlayersListResult {
  source: "ping";
  online?: number;
  max?: number;
  sample: PlayerSample[];
  /** Log-derived roster: joins not yet followed by a leave. Empty unless
   *  this daemon's log pumps saw the joins (adopted servers answer
   *  honestly empty). */
  roster?: PlayerSample[];
  latencyMs: number;
  version?: string;
  motd?: string;
}

// --- errors (§3) ---

export interface ProtocolErrorObject {
  code: string;
  message: string;
  context?: Record<string, unknown>;
  remediation?: string[];
}

// --- JSON-RPC 2.0 envelopes ---
//
// Error responses carry the typed protocol error directly in `error`
// (protocol spec §3: structured errors over the numeric JSON-RPC codes).

export interface JsonRpcRequest {
  jsonrpc: "2.0";
  id: number;
  method: string;
  params?: unknown;
}

export interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: number | string | null;
  result?: unknown;
  error?: ProtocolErrorObject;
}

export interface JsonRpcNotification {
  jsonrpc: "2.0";
  method: string;
  params?: unknown;
}

export type JsonRpcIncoming = JsonRpcResponse | JsonRpcNotification;

// --- software catalog & creation (§7b) ---

export interface CatalogEntry {
  id: string;
  name: string;
  description: string;
  /** Which upstream API family this entry speaks: builds (with
   * published checksums) or loaders (Fabric's meta API). */
  source: "fill" | "fabric-meta";
}

export interface CatalogListResult {
  entries: CatalogEntry[];
}

export interface CatalogVersion {
  id: string;
  javaMajor?: number;
}

export interface CatalogVersionsResult {
  project: string;
  versions: CatalogVersion[];
}

export interface CatalogDownload {
  name: string;
  sha256: string;
  size?: number;
  url: string;
}

export interface CatalogBuild {
  id: number;
  channel: string;
  time?: string;
  download: CatalogDownload;
}

export interface CatalogBuildsResult {
  project: string;
  version: string;
  javaMajor?: number;
  builds: CatalogBuild[];
  /** The Fabric family's stand-in for builds: stable loader versions,
   * newest first; `server.create` pins one via `loader`. */
  loaders?: string[];
}

export interface ServerCreateParams {
  requestId: string;
  serverId: string;
  displayName?: string;
  project: string;
  version: string;
  build?: number;
  /** The Fabric loader version to pin; omit for the newest stable. */
  loader?: string;
  templateId?: string;
  port?: number;
  javaPath?: string;
}

export type ServerCreateResult = BackupJobResult;

// --- java runtimes (§7c) ---

export interface JavaRuntime {
  path: string;
  major: number;
  versionString: string;
  vendor: string;
  managed: boolean;
}

export interface JavaListResult {
  runtimes: JavaRuntime[];
}

export interface JavaInstallParams {
  requestId: string;
  majorVersion: number;
}

export type JavaInstallResult = BackupJobResult;

// --- plugin catalog (§7d, ADR-0012) ---

export interface PluginSearchHit {
  projectId: string;
  slug: string;
  title: string;
  description: string;
  downloads: number;
  iconUrl?: string;
  loaders: string[];
}

export interface PluginsSearchResult {
  /** Where installs land for this server ("plugins" or "mods"). */
  target: string;
  hits: PluginSearchHit[];
}

export interface PluginVersionInfo {
  id: string;
  versionNumber: string;
  gameVersions: string[];
  loaders: string[];
  datePublished?: string;
  fileName?: string;
  sizeBytes?: number;
}

export interface PluginsVersionsResult {
  target: string;
  versions: PluginVersionInfo[];
}

export interface InstalledPlugin {
  fileName: string;
  sizeBytes: number;
  modifiedMs: number;
  symlinkOutside: boolean;
}

export interface PluginsInstalledResult {
  target: string;
  entries: InstalledPlugin[];
}

export type PluginUpdateStatus = "up-to-date" | "update-available" | "unmanaged";

export interface PluginUpdateEntry {
  fileName: string;
  status: PluginUpdateStatus;
  /** Present when the catalog recognized the file's bytes. */
  projectId?: string;
  installedVersion?: string;
  latestVersion?: string;
  /** The pin that applies the update via plugins.install + replace. */
  latestVersionId?: string;
}

export interface PluginsUpdatesResult {
  /** The directory that was checked ("plugins" or "mods"). */
  target: string;
  entries: PluginUpdateEntry[];
}

export interface PluginsInstallResult {
  kind: JobKind;
  job: Job;
}

// --- schedules (§7e, ADR-0014) -------------------------------------------

export type ScheduleSpec =
  | { kind: "interval"; everySecs: number }
  | { kind: "daily"; at: string }
  | { kind: "weekly"; weekdays: string[]; at: string };

export type ScheduleAction =
  | { kind: "restart" }
  | { kind: "backup" }
  | { kind: "command"; line: string };

export interface Schedule {
  id: string;
  name: string;
  spec: ScheduleSpec;
  action: ScheduleAction;
  enabled: boolean;
  createdMs: number;
  lastFiredMs?: number;
}

/** The stored record plus the daemon's computed next-run hint. */
export type ScheduleView = Schedule & { nextRunMs?: number };

export interface SchedulesListResult {
  serverId: string;
  schedules: ScheduleView[];
}

export interface SchedulesCreateResult {
  serverId: string;
  schedule: ScheduleView;
}

export interface SchedulesUpdateResult {
  serverId: string;
  schedule: ScheduleView;
}

// --- publish (§7f, ADR-0017) ---------------------------------------------
// The founder's §40-47 publish: a §41 selection of files, a §42 diff
// against the last publication, the §44/§46 security scan, and a §40
// provider interface. The §43 Dutchmen changelog room stays OFF this
// wire by standing scope — the changelog is an operator-edited string.

export type SelectionRule =
  | { kind: "folder"; path: string }
  | { kind: "file"; path: string }
  | { kind: "glob"; pattern: string };

export interface PublishSelection {
  includes: SelectionRule[];
  excludes: SelectionRule[];
}

export interface PublishConfig {
  selection: PublishSelection;
  providerId: string;
  providerSettings: Record<string, string>;
  title: string;
  description: string;
  version: string;
  changelog: string;
}

export type FileDiffStatus = "added" | "modified" | "removed" | "unchanged";

export interface FileDiffEntry {
  path: string;
  status: FileDiffStatus;
  /** Current size; for removed rows the size it had when published. */
  size?: number;
  sha512?: string;
}

export interface DiffCounts {
  added: number;
  modified: number;
  removed: number;
  unchanged: number;
  /** added + modified + removed — the number the Publish button wears. */
  changed: number;
}

export type SecretSeverity = "critical" | "high" | "medium" | "low";

export interface SecretFinding {
  file: string;
  /** 1-based; 0 marks a file-level finding. */
  line: number;
  kind: string;
  severity: SecretSeverity;
  /** Redacted: a key name or a masked token preview, never the secret. */
  excerpt: string;
  detector: string;
  reviewed: boolean;
}

export interface ScanReport {
  findings: SecretFinding[];
  filesScanned: number;
  filesSkipped: number;
}

export interface PublicationSummary {
  publishedAtMs: number;
  version?: string;
  providerId: string;
  packageSha512: string;
  packageBytes: number;
  fileCount: number;
}

export interface UploadReceipt {
  providerId: string;
  reference: string;
  detail?: string;
  atMs: number;
}

export interface ProviderSettingInfo {
  key: string;
  description: string;
}

export interface ProviderInfo {
  id: string;
  displayName: string;
  needsCredential: boolean;
  credentialEnvVar?: string;
  settings: ProviderSettingInfo[];
}

export interface ProvidersListResult {
  providers: ProviderInfo[];
}

export interface PublishPreviewResult {
  serverId: string;
  config: PublishConfig;
  files: FileDiffEntry[];
  counts: DiffCounts;
  scan: ScanReport;
  blockingCount: number;
  selectedFiles: number;
  selectedBytes: number;
  lastPublication?: PublicationSummary;
}

export interface PublishExecuteResult {
  job: Job;
}

export interface PublishStateResult {
  serverId: string;
  lastPublication?: PublicationSummary;
  receipt?: UploadReceipt;
  packagePresent: boolean;
}

// --- server configuration surfaces (§7g, ADR-0019) --------------------------
// The layered config model (ADR-0007) on the wire: the effective view plus
// per-field provenance, a tri-state patch (absent keeps the override, null
// clears it back to the global default, a value sets it), and the §37
// network probe (desired port vs the server.properties authority, a live
// bind-test, cross-server conflicts).

export type FieldProvenance = "global" | "custom";

export interface EffectiveSettingsView {
  stopTimeoutSecs: number;
  startupTimeoutSecs: number;
  port?: number;
  minMemoryMb?: number;
  maxMemoryMb?: number;
  extraJvmArgs: string[];
  javaPath?: string;
  mcVersion?: string;
  javaMajorRequired?: number;
  backupKeep: number;
}

export interface ProvenanceView {
  stopTimeoutSecs: FieldProvenance;
  startupTimeoutSecs: FieldProvenance;
  port: FieldProvenance;
  minMemoryMb: FieldProvenance;
  maxMemoryMb: FieldProvenance;
  extraJvmArgs: FieldProvenance;
  javaPath: FieldProvenance;
  mcVersion: FieldProvenance;
  javaMajorRequired: FieldProvenance;
  backupKeep: FieldProvenance;
}

export interface ConfigGetResult {
  serverId: string;
  displayName: string;
  /** Server-root relative; absent means the built-in server.jar applies. */
  jar?: string;
  effective: EffectiveSettingsView;
  provenance: ProvenanceView;
}

/**
 * The tri-state patch: `undefined` keeps the current override, `null`
 * clears it (the global default applies again), a value sets it.
 */
export interface ServerSettingsPatch {
  stopTimeoutSecs?: number | null;
  startupTimeoutSecs?: number | null;
  port?: number | null;
  minMemoryMb?: number | null;
  maxMemoryMb?: number | null;
  extraJvmArgs?: string[] | null;
  javaPath?: string | null;
  mcVersion?: string | null;
  javaMajorRequired?: number | null;
  backupKeep?: number | null;
}

export interface ConfigSetPayload {
  displayName?: string;
  jar?: string | null;
  settings: ServerSettingsPatch;
}

export type ConfigSetResult = ConfigGetResult;

export interface NetworkStatusResult {
  serverId: string;
  desiredPort?: number;
  /** The port server.properties names — the boot authority. */
  propertiesPort?: number;
  /** server.properties' server-ip; "" means all interfaces. */
  bindAddress?: string;
  /** Bind-test of the effective port at answer time; null = no port. */
  portAvailable?: boolean;
  /** Other managed servers claiming the same desired port. */
  conflicts: string[];
}
