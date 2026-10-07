//! Imports that exist only under a feature flag.

#[cfg(feature = "json")]
use crate::facade::User;

#[cfg(feature = "yaml")]
use crate::sibling::Helper;

#[cfg(not(feature = "json"))]
use crate::inline_tests::Owner;

// Deliberately ungated, in a file whose other three imports are gated. The flag
// only means something if some edge is not set: a report in which every edge is
// conditional says nothing about which ones are.
use crate::core::user::User as PlainUser;

pub fn gated(user: &User) -> String {
    user.name.clone()
}

pub fn ungated(helper: &Helper) -> Helper {
    Helper
}

pub fn ungated_alias(_user: &PlainUser) {}
