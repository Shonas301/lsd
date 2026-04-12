//! This module defines the [TreeColumns] flag. To set it up from [Cli], a [Config] and its
//! [Default] value, use the [configure_from](TreeColumns::configure_from) method via [Configurable].

use super::Configurable;

use crate::app::Cli;
use crate::config_file::Config;

/// whether to pack top-level tree subtrees horizontally into terminal-width columns
#[derive(Clone, Debug, Copy, PartialEq, Eq, Default)]
pub struct TreeColumns(pub bool);

impl Configurable<Self> for TreeColumns {
    /// Get a potential `TreeColumns` value from [Cli].
    ///
    /// returns `Some(TreeColumns(true))` if the flag was passed, otherwise `None`.
    fn from_cli(cli: &Cli) -> Option<Self> {
        if cli.tree_columns {
            Some(Self(true))
        } else {
            None
        }
    }

    /// Get a potential `TreeColumns` value from a [Config].
    ///
    /// If `Config::tree_columns` has a value, returns it wrapped in `TreeColumns(v)`.
    /// Otherwise returns [None].
    fn from_config(config: &Config) -> Option<Self> {
        config.tree_columns.map(Self)
    }
}

#[cfg(test)]
mod test {
    use clap::Parser;

    use super::TreeColumns;

    use crate::app::Cli;
    use crate::config_file::Config;
    use crate::flags::Configurable;

    #[test]
    fn test_from_cli_none() {
        let argv = ["lsd"];
        let cli = Cli::try_parse_from(argv).unwrap();
        assert_eq!(None, TreeColumns::from_cli(&cli));
    }

    #[test]
    fn test_from_cli_some() {
        let argv = ["lsd", "--tree-columns"];
        let cli = Cli::try_parse_from(argv).unwrap();
        assert_eq!(Some(TreeColumns(true)), TreeColumns::from_cli(&cli));
    }

    #[test]
    fn test_from_config_none() {
        assert_eq!(None, TreeColumns::from_config(&Config::with_none()));
    }

    #[test]
    fn test_from_config_some() {
        let mut c = Config::with_none();
        c.tree_columns = Some(true);
        assert_eq!(Some(TreeColumns(true)), TreeColumns::from_config(&c));
    }

    #[test]
    fn test_default() {
        assert_eq!(TreeColumns(false), TreeColumns::default());
    }

    #[test]
    fn test_configure_from_cli_takes_precedence() {
        let argv = ["lsd", "--tree-columns"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let mut c = Config::with_none();
        c.tree_columns = Some(false);
        assert_eq!(TreeColumns(true), TreeColumns::configure_from(&cli, &c));
    }

    #[test]
    fn test_configure_from_config_fallback() {
        let argv = ["lsd"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let mut c = Config::with_none();
        c.tree_columns = Some(true);
        assert_eq!(TreeColumns(true), TreeColumns::configure_from(&cli, &c));
    }
}
