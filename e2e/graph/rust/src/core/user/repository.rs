use crate::core::user::User;
use crate::sibling::Helper;
use crate::external_reexport::log;
use crate::missing_module;
use std::collections::HashMap;
use std::fmt::Debug as Formattable;

pub fn load(user: &User) -> HashMap<String, User> {
    let _ = Helper::new();
    let _ = user.greeting();
    let _ = log("loaded");
    HashMap::new()
}

pub fn explain<T: Formattable>(_value: &T) {}
