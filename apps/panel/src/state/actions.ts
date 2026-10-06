// Typed operations against the daemon, one function per method the panel
// uses. Errors surface as `ProtocolRequestError` (structured) or transport
// failures; UI layers translate, never re-parse.

import type {
  LifecycleResult,
  LogRangeParams,
  LogRangeResult,
  RegisterServerParams,
  RegisterServerResult,
  RemoveServerParams,
  ServerDetails,
  ServerListResult,
  StdinParams,
  SubscribeResult,
  UpdateServerParams,
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

export async function tailLogs(params: LogRangeParams): Promise<LogRangeResult> {
  return client.request<LogRangeResult>("logs.range", params);
}

export async function subscribeLogs(
  serverId: string,
  handler: Parameters<typeof client.subscribe>[2],
): Promise<{ dispose(): void; result: SubscribeResult | null }> {
  let result: SubscribeResult | null = null;
  const handle = await client.subscribe("logs", serverId, {
    onPayload: (notification) => handler.onPayload(notification),
    onRegistered: (resubscribed) => {
      result = resubscribed;
      handler.onRegistered?.(resubscribed);
    },
  });
  return { dispose: () => handle.dispose(), result };
}
