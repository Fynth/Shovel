// `suspicious_else_formatting` fires on multi-line `if/else` expressions
// inside RSX `class:` attributes. This is a known false positive in macro
// contexts — the `} else {` is on one line, but clippy still flags it.
#![allow(clippy::suspicious_else_formatting)]

mod app;
mod app_state;
mod completion;
mod components;
mod layout;
mod screens;

pub use app::App;
