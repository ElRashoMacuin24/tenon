//! Tenon file formats.
//!
//! STEP goes through the [`Kernel`](tenon_kernel::Kernel) trait (the kernel owns B-rep exchange);
//! mesh formats are written here from a kernel [`Mesh`](tenon_kernel::Mesh). The native project
//! format arrives in M1 (docs/file-format.md).
#![forbid(unsafe_code)]

pub mod stl;
