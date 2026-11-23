//! Agent-native Local Image: the commands the app offers an agent, a headless session that runs
//! them, the MCP server on top of it and the control channel of the running app.
//!
//! The plumbing is the suite's `craft-mcp` (`libs/craft-libs/crates/mcp`, source `local-libs`):
//! the stdio server, the `run --script` steps with `"$N"` references, the
//! `--automation-read-root`/`--automation-write-root` flags and `--connect PORT`. What this module
//! adds is Local Image's own: the command catalogue ([`catalogue`]), the headless session
//! ([`session`]), the MCP handler ([`mcp`]) and the app's loopback control channel ([`control`]).
//!
//! Every entry point is a command: the palette, `command_run`, `run --script` and the control
//! channel all reach the same ids, so an agent sees exactly what the user sees.

pub mod catalogue;
pub mod cli;
pub mod control;
pub mod launch;
pub mod mcp;
pub mod session;
