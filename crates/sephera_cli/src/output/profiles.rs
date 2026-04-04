use crate::context_config::AvailableContextProfiles;

pub fn print_available_profiles(profiles: &AvailableContextProfiles) {
    println!("{}", render_available_profiles(profiles));
}

fn render_available_profiles(profiles: &AvailableContextProfiles) -> String {
    match profiles.source_path.as_ref() {
        Some(source_path) if profiles.profiles.is_empty() => format!(
            "Config: {}\nNo context profiles defined.",
            source_path.display()
        ),
        Some(source_path) => format!(
            "Config: {}\nProfiles:\n{}",
            source_path.display(),
            profiles.profiles.join("\n")
        ),
        None => {
            String::from("No `.sephera.toml` file found for the selected path.")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::context_config::AvailableContextProfiles;

    use super::render_available_profiles;

    #[test]
    fn renders_profile_list_with_source_path() {
        let rendered = render_available_profiles(&AvailableContextProfiles {
            source_path: Some(PathBuf::from("/repo/.sephera.toml")),
            profiles: vec![String::from("debug"), String::from("review")],
        });

        assert!(rendered.contains("Config: /repo/.sephera.toml"));
        assert!(rendered.contains("Profiles:"));
        assert!(rendered.contains("debug\nreview"));
    }

    #[test]
    fn renders_missing_config_message() {
        let rendered = render_available_profiles(&AvailableContextProfiles {
            source_path: None,
            profiles: Vec::new(),
        });

        assert_eq!(
            rendered,
            "No `.sephera.toml` file found for the selected path."
        );
    }
}
