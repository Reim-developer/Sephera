//! Imports that exist only under a feature flag.

#[cfg(feature = "json")]
use crate::facade::User;

#[cfg(feature = "yaml")]
use crate::sibling::Helper;

#[cfg(not(feature = "json"))]
use crate::inline_tests::Owner;

pub fn gated(user: &User) -> String {
    user.name.clone()
}

pub fn ungated(helper: &Helper) -> Helper {
    Helper
}
