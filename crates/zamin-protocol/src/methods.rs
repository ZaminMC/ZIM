//! Method names. One constant per method; never write method strings inline.

pub const DAEMON_HELLO: &str = "daemon.hello";
pub const DAEMON_STATUS: &str = "daemon.status";
/// The heartbeat: a cheap loop-liveness probe. The panel pings it on a
/// timer — a wire that is silently dead, or a session loop that stopped
/// reading, surfaces as a ping timeout instead of a 10 s reply silence.
/// The session loop answers inline (never spawned): THAT is the test.
pub const DAEMON_PING: &str = "daemon.ping";

pub const SERVER_LIST: &str = "server.list";
pub const SERVER_GET: &str = "server.get";
pub const SERVER_REGISTER: &str = "server.register";
pub const SERVER_CREATE: &str = "server.create";
pub const SERVER_UPDATE: &str = "server.update";
pub const SERVER_REMOVE: &str = "server.remove";
pub const SERVER_START: &str = "server.start";
pub const SERVER_STOP: &str = "server.stop";
pub const SERVER_RESTART: &str = "server.restart";
pub const SERVER_DISCOVER: &str = "server.discover";
pub const SERVER_KILL: &str = "server.kill";

/// Discovery's scan-roots surface (founder §64, ADR-0027): the
/// operator-configured roots the daemon walks; the instance dir is
/// implicit and always scanned.
pub const DISCOVERY_ROOTS_GET: &str = "discovery.roots.get";
pub const DISCOVERY_ROOTS_SET: &str = "discovery.roots.set";
pub const SERVER_STDIN: &str = "server.stdin";

// Extensions (founder §56/§57, ADR-0031): the declaration half of the
// permission model — an inventory of installed manifests and the
// problems found reading them. No execution, no contributions yet; the
// result says so itself (contributionsActive).
pub const EXTENSIONS_LIST: &str = "extensions.list";

pub const JOBS_LIST: &str = "jobs.list";
pub const JOBS_GET: &str = "jobs.get";
pub const JOBS_CANCEL: &str = "jobs.cancel";

pub const BACKUP_CREATE: &str = "backup.create";
pub const BACKUP_RESTORE: &str = "backup.restore";
pub const BACKUPS_LIST: &str = "backups.list";

// Specified in v0 as catalog.*; implemented with the software catalog
// (protocol spec §7b).
pub const CATALOG_LIST: &str = "catalog.list";
pub const CATALOG_VERSIONS: &str = "catalog.versions";
pub const CATALOG_BUILDS: &str = "catalog.builds";

// Java runtimes (protocol spec §7c).
pub const JAVA_LIST: &str = "java.list";
pub const JAVA_INSTALL: &str = "java.install";

pub const STREAMS_SUBSCRIBE: &str = "streams.subscribe";
pub const STREAMS_UNSUBSCRIBE: &str = "streams.unsubscribe";
pub const STREAMS_NOTIFICATION: &str = "streams.notification";

pub const LOGS_RANGE: &str = "logs.range";

// Metrics history (protocol spec §6, ADR-0006).
pub const METRICS_RANGE: &str = "metrics.range";

pub const PLAYERS_LIST: &str = "players.list";

pub const PLUGINS_SEARCH: &str = "plugins.search";
pub const PLUGINS_VERSIONS: &str = "plugins.versions";
pub const PLUGINS_INSTALLED: &str = "plugins.installed";
pub const PLUGINS_INSTALL: &str = "plugins.install";
pub const PLUGINS_DELETE: &str = "plugins.delete";
pub const PLUGINS_UPDATES: &str = "plugins.updates";

// Specified in v0, implemented with the file manager (protocol spec §8).
pub const FILES_LIST: &str = "files.list";
pub const FILES_READ: &str = "files.read";
pub const FILES_WRITE: &str = "files.write";
pub const FILES_COMMIT: &str = "files.commit";
pub const FILES_MKDIR: &str = "files.mkdir";
pub const FILES_RENAME: &str = "files.rename";
pub const FILES_DELETE: &str = "files.delete";
// The files slice (spec §8b/§8c, ADR-0021): copy and search complete the
// founder's §32 file manager — a copy that never overwrites, a bounded
// walk that answers "where is it".
pub const FILES_COPY: &str = "files.copy";
pub const FILES_SEARCH: &str = "files.search";

// Specified in v0, implemented with the scheduler (protocol spec §7e,
// ADR-0014): the daemon runs the clock, clients author the rules.
pub const SCHEDULES_LIST: &str = "schedules.list";
pub const SCHEDULES_CREATE: &str = "schedules.create";
pub const SCHEDULES_UPDATE: &str = "schedules.update";
pub const SCHEDULES_DELETE: &str = "schedules.delete";

// Specified in v0 with the audit's read side (ADR-0011): the audit was
// write-only evidence until something could read it back; reads are not
// themselves audited (a listing floods the file without making the
// system safer).
pub const AUDIT_LIST: &str = "audit.list";

// Server configuration surfaces (founder vision §37–39, ADR-0019):
// Startup, Network, and Settings read and write the layered config model
// (ADR-0007); the patch is tri-state so clearing an override back to the
// global default is expressible on the wire.
pub const CONFIG_GET: &str = "config.get";
pub const CONFIG_SET: &str = "config.set";
pub const NETWORK_STATUS: &str = "network.status";

// Publish (founder vision §40–47, §74, ADR-0017): the creator packages a
// chosen selection of a server's files and hands it to a provider. The
// AI changelog room (§43) is deliberately absent from this surface —
// see the scoping note in publish.rs.
pub const PUBLISH_CONFIG_GET: &str = "publish.config.get";
pub const PUBLISH_CONFIG_SET: &str = "publish.config.set";
pub const PUBLISH_PROVIDERS_LIST: &str = "publish.providers.list";
pub const PUBLISH_PREVIEW: &str = "publish.preview";
pub const PUBLISH_EXECUTE: &str = "publish.execute";
pub const PUBLISH_STATE: &str = "publish.state";
pub const PUBLISH_REVIEW_SET: &str = "publish.review.set";
