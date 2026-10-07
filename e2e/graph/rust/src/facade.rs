//! A name re-exported from the crate root.
//
// `use crate::User;` names this, not a file called `User`. Resolving it needs
// the declaration index; before that existed it was reported as a gap.
pub use crate::core::user::User;
pub use crate::sibling::Helper;
