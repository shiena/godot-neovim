mod client;
mod events;
mod handler;

pub use client::NeovimClient;
#[allow(unused_imports)]
pub use client::{IndentOptions, SwitchBufferResult};
#[allow(unused_imports)]
pub use events::{ParseError, RedrawEvent};
pub use handler::{BufEvent, NeovimHandler, NeovimState};
