//! App page surface (D-30): permission gates + inline view guards.
//!
//! [`gate`] ports the page permission layer (`app/permissions/page.py`)
//! and the inline guards in `app/views/page/base.py` that specialize it.
//! Handler routes (issues 320/322/328/332/338) call into it; this module
//! owns no routes yet.

pub mod gate;
