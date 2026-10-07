// Typed operations against the daemon, one function per method the panel
// uses. Errors surface as `ProtocolRequestError` (structured) or transport
// failures; UI layers translate, never re-parse.

import type {
  BackupJobResult,
  BackupsListResult,
  CancelJobParams,
  CatalogBuildsResult,
  CatalogListResult,
  CatalogVersionsResult,
  JavaInstallResult,
  JavaListResult,
  FilesCommitResult,
  Job,
  JobsListResult,
  PlayersListResult,
  FilesListResult,
  FilesReadResult,
  FilesWriteResult,
  LifecycleResult,
  LogRangeParams,
  LogRangeResult,
  MetricsRangeParams,
  MetricsRangeResult,
  RegisterServerParams,
  RegisterServerResult,
  RemoveServerParams,
  ServerCreateParams,
  ServerCreateResult,
  ServerDetails,
  ServerListResult,
  StdinParams,
  StreamCursor,
  SubscribeResult,
  UpdateServerParams,
  PluginsSearchResult,
  PluginsVersionsResult,
  PluginsInstalledResult,
  PluginsInstallResult,
} from "../protocol/types";
import { client } from "./wire";

export function newRequestId(): string {
  return crypto.randomUUID();
}

export async function listServers(): Promise<ServerListResult> {
  return client.request<ServerListResult>("server.list");
}

export async function getServer(serverId: string): Promise<ServerDetails> {
  return client.request<ServerDetails>("server.get", { serverId });
}

export async function registerServer(
  input: Pick<RegisterServerParams, "serverId" | "displayName" | "rootPath">,
): Promise<RegisterServerResult> {
  const params: RegisterServerParams = { ...input, requestId: newRequestId() };
  return client.request<RegisterServerResult>("server.register", params);
}

export async function renameServer(serverId: string, displayName: string): Promise<ServerDetails> {
  const params: UpdateServerParams = { requestId: newRequestId(), serverId, displayName };
  return client.request<ServerDetails>("server.update", params);
}

export async function removeServer(serverId: string): Promise<void> {
  const params: RemoveServerParams = { requestId: newRequestId(), serverId };
  await client.request("server.remove", params);
}

export async function startServer(serverId: string): Promise<LifecycleResult> {
  return client.request<LifecycleResult>("server.start", {
    requestId: newRequestId(),
    serverId,
  });
}

export async function stopServer(serverId: string): Promise<LifecycleResult> {
  return client.request<LifecycleResult>("server.stop", {
    requestId: newRequestId(),
    serverId,
  });
}

export async function restartServer(serverId: string): Promise<LifecycleResult> {
  return client.request<LifecycleResult>("server.restart", {
    requestId: newRequestId(),
    serverId,
  });
}

export async function killServer(serverId: string): Promise<LifecycleResult> {
  return client.request<LifecycleResult>("server.kill", {
    requestId: newRequestId(),
    serverId,
  });
}

export async function sendStdin(serverId: string, line: string): Promise<void> {
  const params: StdinParams = { requestId: newRequestId(), serverId, line };
  await client.request("server.stdin", params);
}

// --- backups & jobs (§7) ---

export async function listBackups(serverId: string): Promise<BackupsListResult> {
  return client.request<BackupsListResult>("backups.list", { serverId });
}

export async function createBackup(serverId: string, label?: string): Promise<BackupJobResult> {
  const params = {
    requestId: newRequestId(),
    serverId,
    ...(label === undefined || label === "" ? {} : { label }),
  };
  return client.request<BackupJobResult>("backup.create", params);
}

export async function restoreBackup(serverId: string, backupId: string): Promise<BackupJobResult> {
  const params: { requestId: string; serverId: string; backupId: string } = {
    requestId: newRequestId(),
    serverId,
    backupId,
  };
  return client.request<BackupJobResult>("backup.restore", params);
}

export async function listJobs(): Promise<JobsListResult> {
  return client.request<JobsListResult>("jobs.list");
}

export async function cancelJob(jobId: string): Promise<Job> {
  const params: CancelJobParams = { requestId: newRequestId(), jobId };
  return client.request<Job>("jobs.cancel", params);
}

// --- players (§5) ---

export async function listPlayers(serverId: string): Promise<PlayersListResult> {
  return client.request<PlayersListResult>("players.list", { serverId });
}

// --- files (§8) ---

export async function listFiles(params: {
  serverId: string;
  path: string;
  offset?: number;
  limit?: number;
}): Promise<FilesListResult> {
  return client.request<FilesListResult>("files.list", params);
}

export async function readFileChunk(params: {
  serverId: string;
  path: string;
  offset: number;
  maxBytes: number;
}): Promise<FilesReadResult> {
  return client.request<FilesReadResult>("files.read", params);
}

export async function writeFileChunk(params: {
  serverId: string;
  stagingId?: string;
  content: string;
}): Promise<FilesWriteResult> {
  return client.request<FilesWriteResult>("files.write", params);
}

export async function commitFile(params: {
  serverId: string;
  stagingId: string;
  target: string;
}): Promise<FilesCommitResult> {
  return client.request<FilesCommitResult>("files.commit", params);
}

export async function mkdir(serverId: string, path: string): Promise<void> {
  await client.request("files.mkdir", { serverId, path });
}

export async function renameEntry(serverId: string, from: string, to: string): Promise<void> {
  await client.request("files.rename", { serverId, from, to });
}

export async function deleteEntry(serverId: string, path: string): Promise<void> {
  await client.request("files.delete", { serverId, path });
}

/** Read a whole file through chunked reads (spec §8), 512 KiB at a time. */
export async function readWholeFile(serverId: string, path: string): Promise<Uint8Array> {
  const READ_CHUNK = 512 * 1024;
  const parts: Uint8Array[] = [];
  let offset = 0;
  for (;;) {
    const chunk = await readFileChunk({ serverId, path, offset, maxBytes: READ_CHUNK });
    const bytes = base64ToBytes(chunk.data);
    parts.push(bytes);
    offset += bytes.length;
    if (chunk.eof) break;
  }
  const total = parts.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(total);
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

export function base64ToBytes(value: string): Uint8Array {
  const binary = atob(value);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) out[i] = binary.charCodeAt(i);
  return out;
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

/** Write a whole file through the staged upload (spec §8), 512 KiB chunks. */
export async function writeWholeFile(
  serverId: string,
  path: string,
  bytes: Uint8Array,
): Promise<void> {
  const WRITE_CHUNK = 512 * 1024;
  let stagingId: string | undefined;
  for (let at = 0; ; at += WRITE_CHUNK) {
    const slice = bytes.subarray(at, Math.min(at + WRITE_CHUNK, bytes.length));
    // An empty file still stages one empty chunk so the handle exists.
    const result = await writeFileChunk({
      serverId,
      stagingId,
      content: bytesToBase64(slice),
    });
    stagingId = result.stagingId;
    if (slice.length < WRITE_CHUNK) break;
  }
  await commitFile({ serverId, stagingId, target: path });
}

export async function rangeLogs(params: LogRangeParams): Promise<LogRangeResult> {
  return client.request<LogRangeResult>("logs.range", params);
}

export async function subscribeLogs(
  serverId: string,
  handler: Parameters<typeof client.subscribe>[2],
  /** Explicit first-subscribe cursor. A logs cursor starts the
   *  subscription live-only (no ring replay) — the log viewer pairs it
   *  with a file-backed range read; the console leaves it absent and
   *  takes the ring replay as its opening batch. */
  initialCursor?: StreamCursor,
): Promise<{ dispose(): void; result: SubscribeResult | null }> {
  let result: SubscribeResult | null = null;
  const handle = await client.subscribe(
    "logs",
    serverId,
    {
      onPayload: (notification) => handler.onPayload(notification),
      onRegistered: (resubscribed) => {
        result = resubscribed;
        handler.onRegistered?.(resubscribed);
      },
    },
    initialCursor,
  );
  return { dispose: () => handle.dispose(), result };
}

export async function metricsRange(
  params: MetricsRangeParams,
): Promise<MetricsRangeResult> {
  return client.request<MetricsRangeResult>("metrics.range", params);
}

export async function subscribeMetrics(
  serverId: string,
  handler: Parameters<typeof client.subscribe>[2],
): Promise<{ dispose(): void }> {
  const handle = await client.subscribe("metrics", serverId, handler);
  return { dispose: () => handle.dispose() };
}

// --- software catalog & creation (§7b) ---

export async function catalogList(): Promise<CatalogListResult> {
  return client.request<CatalogListResult>("catalog.list");
}

export async function catalogVersions(project: string): Promise<CatalogVersionsResult> {
  return client.request<CatalogVersionsResult>("catalog.versions", { project });
}

export async function catalogBuilds(
  project: string,
  version: string,
): Promise<CatalogBuildsResult> {
  return client.request<CatalogBuildsResult>("catalog.builds", { project, version });
}

export async function createServer(
  params: Omit<ServerCreateParams, "requestId">,
): Promise<ServerCreateResult> {
  return client.request<ServerCreateResult>("server.create", {
    ...params,
    requestId: newRequestId(),
  });
}

// --- java runtimes (§7c) ---

export async function listJava(): Promise<JavaListResult> {
  return client.request<JavaListResult>("java.list");
}

export async function installJava(majorVersion: number): Promise<JavaInstallResult> {
  return client.request<JavaInstallResult>("java.install", {
    requestId: newRequestId(),
    majorVersion,
  });
}

// --- plugin catalog (§7d, ADR-0012) ---

export async function pluginsSearch(
  serverId: string,
  query: string,
): Promise<PluginsSearchResult> {
  return client.request<PluginsSearchResult>("plugins.search", { serverId, query, limit: 20 });
}

export async function pluginsVersions(
  serverId: string,
  projectId: string,
): Promise<PluginsVersionsResult> {
  return client.request<PluginsVersionsResult>("plugins.versions", { serverId, projectId });
}

export async function pluginsInstalled(serverId: string): Promise<PluginsInstalledResult> {
  return client.request<PluginsInstalledResult>("plugins.installed", { serverId });
}

export async function pluginsInstall(
  serverId: string,
  projectId: string,
  versionId?: string,
  replace?: boolean,
): Promise<PluginsInstallResult> {
  return client.request<PluginsInstallResult>("plugins.install", {
    serverId,
    projectId,
    ...(versionId === undefined ? {} : { versionId }),
    // The update rule (ADR-0012): the daemon refuses a differing file with
    // PLUGIN_EXISTS; `replace` is the operator's explicit overwrite.
    ...(replace ? { replace: true } : {}),
  });
}

export async function pluginsDelete(serverId: string, fileName: string): Promise<void> {
  await client.request("plugins.delete", { serverId, fileName });
}
