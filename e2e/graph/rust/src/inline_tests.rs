//! `super::` inside an inline module.

use crate::sibling::Helper;

pub struct Owner {
    pub helper: Helper,
}

#[cfg(test)]
mod tests {
    use super::Owner;
    // `Owner` is declared in this file, so naming it through `super` is a real
    // reference to this file.
    use super::Owner as Renamed;

    // `helper_module` does not exist here. A `super::` path that names nothing
    // in this file is a gap, and falling back to "the source file itself"
    // invents a dependency.
    #[allow(unused_imports)]
    use super::helper_module;

    // A file this one does not declare: `super` from an inline module is still
    // this file, so this is a gap too.
    #[allow(unused_imports)]
    use super::crate::sibling;

    pub fn owns(_owner: &Owner) -> Renamed {
        Renamed
    }
}
