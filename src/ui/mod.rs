//! Terminal UI module

pub mod footer;
pub mod header;
pub mod layout;
pub mod pages;
pub mod theme;
mod theme_builtins;
pub mod widgets;

pub use layout::draw;
