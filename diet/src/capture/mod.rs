//! Capture: what a lane is allowed to write into the working object.
//!
//! An entry written into the working object carries **capture authority**:
//! downstream turns treat it as true. That is what makes this directory
//! different from `formats/` -- a format says what a text means, and capture
//! says what may be believed.

pub mod grounded;
pub mod mechanical;
pub mod router;
