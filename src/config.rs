use std::collections::HashSet;

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::{endpoint::EndpointLabel, error::Error};

/// A group of endpoints, identified by their labels.
/// These endpoints will be allocated together.
/// This means that controlling one of them allows control of all of them.
///
/// Groups must uphold some invariants (validated at runtime):
///     - The endpoints must be uniquely found in a single group.
///     - A group is non-empty.
///     - A group only has members of the same variant.
#[derive(Debug, Serialize, Deserialize)]
pub struct Group(pub(crate) HashSet<EndpointLabel>);

impl Group {
    pub(crate) fn is_mock_group(&self) -> bool {
        let member = self.0.iter().last().expect("Groups are non-empty");

        matches!(member, EndpointLabel::Mock(_))
    }
}

impl From<Vec<EndpointLabel>> for Group {
    fn from(labels: Vec<EndpointLabel>) -> Self {
        let mut hs = HashSet::new();
        hs.extend(labels);
        Self(hs)
    }
}

/// The configuration used for running the server.
// TODO: Enforce groups only contain the same endpoint variant?
// TODO: Enforce groups are non-empty
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Should all found serial ports be opened automatically?
    ///
    /// For example on Windows this will open all "COMn" ports found.
    /// On Unix, this will TODO.
    pub auto_open_serial_ports: bool,

    /// Logical groupings of endpoints.
    /// See [`Group`].
    pub groups: Vec<Group>,
}

impl Config {
    fn check_duplicates_across_groups(&self) -> Result<(), Error> {
        let duplicates = self
            .groups
            .iter()
            .flat_map(|g| &g.0)
            .duplicates()
            .collect::<Vec<_>>();

        if duplicates.is_empty() {
            Ok(())
        } else {
            Err(Error::BadConfig(format!("Groups represent controllable units. This breaks if endpoints are shared across groups. Duplicates: {duplicates:?}")))
        }
    }

    fn check_empty_within_group(&self) -> Result<(), Error> {
        // Not defining any groups is ok.
        // The problem is defining a group and then not
        // putting anything in it.
        if self.groups.is_empty() {
            return Ok(());
        }

        for (index, group) in self.groups.iter().enumerate() {
            if group.0.is_empty() {
                return Err(Error::BadConfig(format!("The group with index {index} (zero indexed) is empty. If defining groups, please put entries into it.")));
            }
        }

        Ok(())
    }

    fn check_group_variant_homogeneity(&self) -> Result<(), Error> {
        for (index, group) in self.groups.iter().enumerate() {
            let (mocks, ttys): (Vec<_>, Vec<_>) = group
                .0
                .iter()
                .partition(|label| matches!(label, &EndpointLabel::Mock(_)));

            match (mocks.len(), ttys.len()) {
                (0, 0) => unreachable!(),
                (_, 0) | (0, _) => continue,
                (_, _) => return Err(
                    Error::BadConfig(
                        format!("The group with index {index} (zero indexed) has endpoints of different variants, please only put the same endpoint variant type within the same group. The problematic group: `{group:?}`."))
                ),
            }
        }

        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), Error> {
        self.check_empty_within_group()?;
        self.check_group_variant_homogeneity()?;
        self.check_duplicates_across_groups()?;

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auto_open_serial_ports: true,
            groups: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn serialize() {
        let mut g1 = HashSet::new();
        g1.extend(vec![
            EndpointLabel::Tty("COM0".into()),
            EndpointLabel::Tty("COM1".into()),
            EndpointLabel::Tty("COM2".into()),
        ]);

        let mut g2 = HashSet::new();
        g2.extend(vec![
            EndpointLabel::mock("/dev/ttyACM123"),
            EndpointLabel::mock("some-mock"),
            EndpointLabel::mock("another-mock"),
        ]);

        let c = Config {
            groups: vec![Group(g1), Group(g2)],
            auto_open_serial_ports: true,
        };

        println!(
            "{}",
            ron::ser::to_string_pretty(&c, ron::ser::PrettyConfig::default()).unwrap()
        );
    }

    #[test]
    fn deserialize() {
        let input = r#"
(
    auto_open_serial_ports: true,
    groups: [
        ([
            Tty("COM2"),
            Tty("COM0"),
            Tty("COM1"),
        ]),
        ([
            Mock("another-mock"),
            Mock("some-mock"),
            Mock("/dev/ttyACM123"),
        ]),
    ],
)"#;
        ron::from_str::<Config>(input).unwrap();
    }

    #[test]
    fn bad_config_duplicates() {
        let mut g1 = HashSet::new();
        g1.extend(vec![
            EndpointLabel::Tty("COM0".into()),
            EndpointLabel::Tty("COM1".into()),
            EndpointLabel::Tty("COM2".into()),
            EndpointLabel::Tty("COM3".into()),
            EndpointLabel::Tty("COM4".into()),
            EndpointLabel::Tty("COM5".into()),
            EndpointLabel::Tty("COM6".into()),
        ]);

        let mut g2 = HashSet::new();
        g2.extend(vec![
            EndpointLabel::Tty("COM10".into()),
            EndpointLabel::Tty("COM11".into()),
            EndpointLabel::Tty("COM4".into()), // Duplicate!
            EndpointLabel::Tty("COM5".into()), // Duplicate!
            EndpointLabel::Tty("COM12".into()),
        ]);

        let c = Config {
            groups: vec![Group(g1), Group(g2)],
            ..Default::default()
        };

        let err = c.validate().unwrap_err().try_into_bad_config().unwrap();

        // Let's do some assertions that enforces our error messages to at least be decent.
        assert!(!err.contains("COM11"));

        assert!(err.contains("COM4"));
        assert!(err.contains("COM5"));
    }

    #[test]
    fn bad_config_empty() {
        let mut g1 = HashSet::new();
        g1.extend(vec![EndpointLabel::Tty("COM0".into())]);

        let mut g2 = HashSet::new();
        g2.extend(vec![EndpointLabel::Tty("COM2".into())]);

        let g3 = HashSet::new();

        let c = Config {
            groups: vec![Group(g1), Group(g2), Group(g3)],
            ..Default::default()
        };

        let err = c.validate().unwrap_err().try_into_bad_config().unwrap();

        // Error message countains the index of our bad group
        assert!(err.contains("index 2"));
    }

    #[test]
    fn bad_config_homogeneity() {
        let mut g1 = HashSet::new();
        g1.extend(vec![EndpointLabel::Tty("COM0".into())]);

        let mut g2 = HashSet::new();
        g2.extend(vec![EndpointLabel::Mock("Mock0".into())]);

        let mut g3 = HashSet::new();
        g3.extend(vec![
            EndpointLabel::Mock("Mock1".into()),
            EndpointLabel::Tty("COM1".into()),
        ]);

        let c = Config {
            groups: vec![Group(g1), Group(g2), Group(g3)],
            ..Default::default()
        };

        let err = c.validate().unwrap_err().try_into_bad_config().unwrap();

        dbg!(&err);

        // Error message mentions the members of the problematic group
        assert!(err.contains("Mock1"));
        assert!(err.contains("COM1"));
    }
}
