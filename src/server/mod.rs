pub mod detect;
pub mod discovery;
pub mod java;
pub mod mcsm;
pub mod perms;
pub mod start_command;

pub use detect::{PlatformKind, ServerJarInfo};
pub use discovery::{discover, Server, ServerOrigin};
pub use perms::Access;
