//! mcplug — terminal plugin and server-jar manager for Minecraft servers.

pub mod cli;
pub mod config;
pub mod http;
pub mod platform;
pub mod plugins;
pub mod error;
pub mod server;
pub mod util;

pub use error::{Error, Result};
