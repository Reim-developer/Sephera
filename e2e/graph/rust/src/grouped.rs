//! Several import shapes that share one statement.

use crate::core::user::{User, User as Renamed};
use crate::core::{user, Repository};
use crate::sibling::{self, Helper};
use crate::{facade, inline_tests, sibling as sib};

pub fn all(
    _a: &User,
    _b: &Renamed,
    _c: &user::User,
    _d: &Repository,
    _e: &sibling::Helper,
    _f: &sib::Helper,
    _g: &facade::User,
    _h: &inline_tests::Owner,
) {
}
