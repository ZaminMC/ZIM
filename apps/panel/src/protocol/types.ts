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
  | "java.install";
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
}

export interface ServerCreateParams {
  requestId: string;
  serverId: string;
  displayName?: string;
  project: string;
  version: string;
  build?: number;
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
