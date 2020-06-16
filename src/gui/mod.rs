//! GUI frontend implementations.

pub mod minifb;

// NOTE: this indirection could be avoided once either feature(trait_alias) or
// feature(type_alias_impl_trait) land.
pub type RenderCallback = Box<dyn FnMut(&mut Vec<u32>) -> (usize, usize) + Send>;

pub type KeyCallback = Box<dyn FnMut(bool) + Send>;
