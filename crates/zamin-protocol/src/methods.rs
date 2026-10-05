//! Method names. One constant per method; never write method strings inline.

pub const DAEMON_HELLO: &str = "daemon.hello";
pub const DAEMON_STATUS: &str = "daemon.status";

pub const SERVER_LIST: &str = "server.list";
pub const SERVER_GET: &str = "server.get";
pub const SERVER_REGISTER: &str = "server.register";
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

pub const STREAMS_SUBSCRIBE: &str = "streams.subscribe";
pub const STREAMS_UNSUBSCRIBE: &str = "streams.unsubscribe";
pub const STREAMS_NOTIFICATION: &str = "streams.notification";

pub const LOGS_RANGE: &str = "logs.range";

// Specified in v0, implemented with the file manager (protocol spec §8).
pub const FILES_LIST: &str = "files.list";
pub const FILES_READ: &str = "files.read";
pub const FILES_WRITE: &str = "files.write";
pub const FILES_COMMIT: &str = "files.commit";
pub const FILES_MKDIR: &str = "files.mkdir";
pub const FILES_RENAME: &str = "files.rename";
pub const FILES_DELETE: &str = "files.delete";
