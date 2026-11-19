//! The settings file, state file and themes in pulsr's configuration directory
//! (`$XDG_CONFIG_HOME/pulsr` or `~/.config/pulsr`), as lines of `key = value`.
//! Nothing here stops pulsr starting: what cannot be used is left out with a warning.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use super::theme::Theme;

/// What the keys chose, by setting.
pub type State = BTreeMap<String, String>;

/// A `key = value` line of the settings file.
#[derive(Debug, PartialEq)]
pub struct Setting {
    pub line: usize,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Default)]
pub struct Config {
    /// Where it was read from, and where the state is written.
    pub dir: Option<PathBuf>,
    pub settings: Vec<Setting>,
    pub state: State,
    /// Whether the state was written after the settings file; the later one wins.
    pub state_is_newer: bool,
    /// In the order of their file names.
    pub themes: Vec<Theme>,
    pub warnings: Vec<String>,
}

/// The non-blank, non-comment lines of `text`, numbered from 1; `None` for a line
/// that is not `key = value`.
pub fn entries(text: &str) -> impl Iterator<Item = (usize, Option<(&str, &str)>)> {
    text.lines().enumerate().filter_map(|(index, line)| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let entry = line
            .split_once('=')
            .map(|(key, value)| (key.trim(), value.trim()))
            .filter(|(key, value)| !key.is_empty() && !value.is_empty());
        Some((index + 1, entry))
    })
}

/// pulsr's configuration directory, from `XDG_CONFIG_HOME` or `HOME`.
pub fn dir() -> Option<PathBuf> {
    let absolute = |name: &str| std::env::var_os(name).map(PathBuf::from).filter(|path| path.is_absolute());
    directory(absolute("XDG_CONFIG_HOME"), absolute("HOME"))
}

fn directory(xdg_config_home: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    xdg_config_home.or_else(|| home.map(|home| home.join(".config"))).map(|base| base.join("pulsr"))
}

/// Reads the settings and themes in `dir`. Neither has to be there.
pub fn read(dir: &Path) -> Config {
    let mut config = Config { dir: Some(dir.to_path_buf()), ..Config::default() };
    if let Ok(text) = std::fs::read_to_string(dir.join("state")) {
        config.state =
            entries(&text).filter_map(|(_, entry)| entry).map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let written = |name: &str| std::fs::metadata(dir.join(name)).and_then(|file| file.modified()).ok();
        config.state_is_newer = written("state") >= written("config");
    }
    if let Ok(text) = std::fs::read_to_string(dir.join("config")) {
        for (line, entry) in entries(&text) {
            match entry {
                Some((key, value)) => {
                    config.settings.push(Setting { line, key: key.to_string(), value: value.to_string() });
                }
                None => config.warnings.push(format!("config line {line}: expected key = value")),
            }
        }
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("themes"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "theme"))
        .collect();
    files.sort();
    for path in files {
        let file = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let theme = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot be read: {error}"))
            .and_then(|text| Theme::parse(&name, &text));
        match theme {
            Ok(theme) => config.themes.push(theme),
            Err(error) => config.warnings.push(format!("theme {file} left out: {error}")),
        }
    }
    config
}

/// Writes the state file in `dir`, making the directory if need be.
pub fn write_state(dir: &Path, state: &State) -> std::io::Result<()> {
    let mut text = String::from("# Written by pulsr on quitting: what the keys last chose.\n");
    for (key, value) in state {
        text.push_str(&format!("{key} = {value}\n"));
    }
    std::fs::create_dir_all(dir)?;
    // Written beside it and renamed into place, so the state file is never half written.
    let unfinished = dir.join("state.new");
    std::fs::write(&unfinished, text)?;
    std::fs::rename(unfinished, dir.join("state"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_the_lines_that_say_something() {
        let text = "# a comment\n\n theme = nord \nfull=true\nnonsense\n = x\ny =\n";
        let found: Vec<_> = entries(text).collect();
        assert_eq!(found, [(3, Some(("theme", "nord"))), (4, Some(("full", "true"))), (5, None), (6, None), (7, None)]);
    }

    #[test]
    fn the_directory_follows_xdg_and_falls_back_to_dot_config() {
        let (xdg, home) = (PathBuf::from("/x"), PathBuf::from("/home/me"));
        assert_eq!(directory(Some(xdg), Some(home.clone())), Some(PathBuf::from("/x/pulsr")));
        assert_eq!(directory(None, Some(home)), Some(PathBuf::from("/home/me/.config/pulsr")));
        assert_eq!(directory(None, None), None);
    }

    #[test]
    fn settings_and_themes_are_read_and_what_is_broken_is_left_out() {
        let dir = std::env::temp_dir().join(format!("pulsr-config-test-{}", std::process::id()));
        let themes = dir.join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(dir.join("config"), "theme = godzilla\noops\nfps = 30\n").unwrap();
        std::fs::write(themes.join("godzilla.theme"), "background = #0b1410\ngradient atomic = #1f6f4a #39d98a\n")
            .unwrap();
        std::fs::write(themes.join("broken.theme"), "background = green\n").unwrap();
        std::fs::write(themes.join("notes.txt"), "not a theme").unwrap();

        let config = read(&dir);
        std::fs::remove_dir_all(&dir).unwrap();

        let settings: Vec<_> = config.settings.iter().map(|s| (s.line, s.key.as_str(), s.value.as_str())).collect();
        assert_eq!(settings, [(1, "theme", "godzilla"), (3, "fps", "30")]);
        assert_eq!(config.themes.len(), 1);
        assert_eq!(config.themes[0].name, "godzilla");
        assert_eq!(config.warnings.len(), 2);
        assert_eq!(config.warnings[0], "config line 2: expected key = value");
        assert!(config.warnings[1].starts_with("theme broken.theme left out: line 1:"), "{}", config.warnings[1]);

        assert!(config.state.is_empty());

        // A directory that is not there is no settings and no themes.
        let config = read(&dir);
        assert!(config.settings.is_empty() && config.themes.is_empty() && config.warnings.is_empty());
    }

    #[test]
    fn the_state_is_read_back_as_it_was_written() {
        let dir = std::env::temp_dir().join(format!("pulsr-state-test-{}", std::process::id())).join("pulsr");
        let state: State = [("theme", "nord"), ("full", "true")].map(|(k, v)| (k.to_string(), v.to_string())).into();
        write_state(&dir, &state).unwrap();
        let config = read(&dir);
        assert_eq!(config.state, state);
        // With no settings file there is nothing for it to be older than.
        assert!(config.state_is_newer);
        assert!(!dir.join("state.new").exists());
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }
}
