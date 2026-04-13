//! This module defines the [MaxShown] flag. To set it up from [Cli], a [Config] and its
//! [Default] value, use the [configure_from](MaxShown::configure_from) method via [Configurable].

use super::Configurable;

use crate::app::Cli;
use crate::config_file::Config;

/// max number of items to show per directory level in tree layout.
///
/// a negative cli/config value resolves to [MaxShown::Auto], which is later
/// replaced at render time by the top-level entry count — so every depth
/// truncates to whatever the root level shows.
#[derive(Clone, Debug, Copy, PartialEq, Eq, Default)]
pub enum MaxShown {
    #[default]
    Unset,
    Auto,
    Count(usize),
}

impl MaxShown {
    /// resolve to a concrete Option<usize> cap. `top_count` is used when self is Auto.
    pub fn resolve(self, top_count: usize) -> Option<usize> {
        match self {
            MaxShown::Unset => None,
            MaxShown::Auto => Some(top_count),
            MaxShown::Count(n) => Some(n),
        }
    }

    /// true when the user gave any explicit value (positive, zero, or negative/auto).
    pub fn is_set(self) -> bool {
        !matches!(self, MaxShown::Unset)
    }
}

fn from_signed(n: i64) -> MaxShown {
    if n < 0 {
        MaxShown::Auto
    } else {
        MaxShown::Count(n as usize)
    }
}

impl Configurable<Self> for MaxShown {
    /// Get a potential `MaxShown` value from [Cli].
    ///
    /// Negative values map to [MaxShown::Auto]; nonnegative values map to [MaxShown::Count].
    /// Returns [None] when `--max-shown` is not on the command line.
    fn from_cli(cli: &Cli) -> Option<Self> {
        cli.max_shown.map(from_signed)
    }

    /// Get a potential `MaxShown` value from a [Config].
    ///
    /// Same negative/nonnegative mapping as [from_cli](Self::from_cli).
    fn from_config(config: &Config) -> Option<Self> {
        config.max_shown.map(from_signed)
    }
}

#[cfg(test)]
mod test {
    use clap::Parser;

    use super::MaxShown;

    use crate::app::Cli;
    use crate::config_file::Config;
    use crate::flags::Configurable;

    #[test]
    fn test_from_cli_none() {
        let argv = ["lsd"];
        let cli = Cli::try_parse_from(argv).unwrap();
        assert_eq!(None, MaxShown::from_cli(&cli));
    }

    #[test]
    fn test_from_cli_some() {
        let argv = ["lsd", "--max-shown", "3"];
        let cli = Cli::try_parse_from(argv).unwrap();
        assert_eq!(Some(MaxShown::Count(3)), MaxShown::from_cli(&cli));
    }

    #[test]
    fn test_from_cli_zero_is_count() {
        let argv = ["lsd", "--max-shown", "0"];
        let cli = Cli::try_parse_from(argv).unwrap();
        assert_eq!(Some(MaxShown::Count(0)), MaxShown::from_cli(&cli));
    }

    #[test]
    fn test_from_cli_negative_is_auto() {
        let argv = ["lsd", "--max-shown", "-1"];
        let cli = Cli::try_parse_from(argv).unwrap();
        assert_eq!(Some(MaxShown::Auto), MaxShown::from_cli(&cli));
    }

    #[test]
    fn test_from_cli_large_negative_is_auto() {
        let argv = ["lsd", "--max-shown", "-42"];
        let cli = Cli::try_parse_from(argv).unwrap();
        assert_eq!(Some(MaxShown::Auto), MaxShown::from_cli(&cli));
    }

    #[test]
    fn test_from_config_none() {
        assert_eq!(None, MaxShown::from_config(&Config::with_none()));
    }

    #[test]
    fn test_from_config_some() {
        let mut c = Config::with_none();
        c.max_shown = Some(5);
        assert_eq!(Some(MaxShown::Count(5)), MaxShown::from_config(&c));
    }

    #[test]
    fn test_from_config_negative_is_auto() {
        let mut c = Config::with_none();
        c.max_shown = Some(-1);
        assert_eq!(Some(MaxShown::Auto), MaxShown::from_config(&c));
    }

    #[test]
    fn test_default() {
        assert_eq!(MaxShown::Unset, MaxShown::default());
    }

    #[test]
    fn test_configure_from_cli_takes_precedence() {
        let argv = ["lsd", "--max-shown", "7"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let mut c = Config::with_none();
        c.max_shown = Some(2);
        assert_eq!(MaxShown::Count(7), MaxShown::configure_from(&cli, &c));
    }

    #[test]
    fn test_configure_from_config_fallback() {
        let argv = ["lsd"];
        let cli = Cli::try_parse_from(argv).unwrap();
        let mut c = Config::with_none();
        c.max_shown = Some(4);
        assert_eq!(MaxShown::Count(4), MaxShown::configure_from(&cli, &c));
    }

    #[test]
    fn test_resolve_unset_returns_none() {
        assert_eq!(None, MaxShown::Unset.resolve(9));
    }

    #[test]
    fn test_resolve_auto_uses_top_count() {
        assert_eq!(Some(9), MaxShown::Auto.resolve(9));
        assert_eq!(Some(0), MaxShown::Auto.resolve(0));
    }

    #[test]
    fn test_resolve_count_ignores_top_count() {
        assert_eq!(Some(3), MaxShown::Count(3).resolve(9));
    }

    #[test]
    fn test_is_set() {
        assert!(!MaxShown::Unset.is_set());
        assert!(MaxShown::Auto.is_set());
        assert!(MaxShown::Count(0).is_set());
        assert!(MaxShown::Count(5).is_set());
    }
}
