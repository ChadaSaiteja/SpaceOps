//! C ABI boundary (ADR-001). Only crate allowed to use unsafe for the Rust<->C# bridge
//! (as opposed to `scanner`'s `winfs` module, which uses unsafe for Rust<->Win32 calls;
//! see ADR-003 note). Every exported function is panic-guarded (ADR-004/007): a panic
//! anywhere in scanner logic never unwinds across this boundary.
//!
//! Note on panic safety and rayon: `scanner::scan`'s internal parallelism is built on
//! `rayon`'s structured `par_iter`/`reduce`, which re-raises any worker-thread panic on
//! the thread that calls `.reduce()` (i.e. back inside this same call stack). A single
//! `catch_unwind` at each exported function's entry point is therefore sufficient; no
//! additional per-callback-site guarding inside `scanner` is needed.
//!
//! Module map: `error` (codes, panic guard, logging), `handles` (opaque handle types),
//! `marshal` (UTF-16 helpers), then one module per feature. Every `#[no_mangle]` export
//! lives in a feature module and is re-exported here; csbindgen reads the modules in the
//! order listed in `build.rs`, which keeps `NativeMethods.g.cs` stable.

mod apps;
mod cleanup;
mod developer;
mod error;
mod handles;
mod index;
mod marshal;
mod scan;
mod search;
#[cfg(test)]
mod test_util;
mod tree;
mod treemap;

pub use apps::*;
pub use cleanup::*;
pub use developer::*;
pub use error::*;
pub use handles::*;
pub use index::*;
pub use scan::*;
pub use search::*;
pub use tree::*;
pub use treemap::*;
