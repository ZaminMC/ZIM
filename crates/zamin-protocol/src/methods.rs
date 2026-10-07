//! Method names. One constant per method; never write method strings inline.

pub const DAEMON_HELLO: &str = "daemon.hello";
pub const DAEMON_STATUS: &str = "daemon.status";

pub const SERVER_LIST: &str = "server.list";
pub const SERVER_GET: &str = "server.get";
pub const SERVER_REGISTER: &str = "server.register";
pub const SERVER_CREATE: &str = "server.create";
pub const SERVER_UPDATE: &str = "server.update";
pub const SERVER_REMOVE: &str = "server.remove";
pub const SERVER_START: &str = "server.start";
pub const SERVER_STOP: &str = "server.stop";
pub const SERVER_RESTART: &str = "server.restart";
pub const SERVER_KILL: &str = "server.kill";
pub const SERVER_STDIN: &str = "server.stdin";

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
