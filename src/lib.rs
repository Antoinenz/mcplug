//! mcplug — terminal plugin and server-jar manager for Minecraft servers.

pub mod backup;
pub mod cli;
pub mod config;
pub mod control;
pub mod http;
pub mod jobs;
pub mod lockfile;
pub mod ops;
pub mod platform;
pub mod plugins;
pub mod error;
pub mod server;
pub mod sources;
pub mod transaction;
pub mod tui;
pub mod util;

pub use error::{Error, Result};
