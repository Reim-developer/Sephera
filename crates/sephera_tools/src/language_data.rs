mod load;
mod model;
mod paths;
mod render;
#[cfg(test)]
mod tests;
mod validate;

pub use load::{load_registry_from_file, load_registry_from_yaml};
pub use model::{
    CommentStyleSpec, LanguageRegistry, LanguageSpec, RawLanguageRegistry,
    RawLanguageSpec,
};
pub use paths::{
    DEFAULT_GENERATED_LANGUAGE_DATA_RELATIVE, DEFAULT_LANGUAGE_CONFIG_RELATIVE,
    default_generated_language_data_path, default_language_config_path,
};
pub use render::{generate_language_data_file, render_language_module};
