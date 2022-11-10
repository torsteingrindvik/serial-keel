use std::collections::HashSet;

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::{
    endpoint::{EndpointId, Label},
    error::Error,
};

/// A group of endpoints, identified by their ids.
/// These endpoints will be allocated together.
/// This means that controlling one of them allows control of all of them.
///
/// Groups must uphold some invariants (validated at runtime):
///     - The endpoints must be uniquely found in a single group.
///     - A group is non-empty.
///     - A group only has members of the same variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub(crate) label: Option<Label>,
    pub(crate) endpoint_ids: HashSet<EndpointId>,
}

impl Group {
    pub(crate) fn is_mock_group(&self) -> bool {
        let member = self
            .endpoint_ids
            .iter()
            .last()
            .expect("Groups are non-empty");

        matches!(member, EndpointId::Mock(_))
    }
}

impl From<Vec<EndpointId>> for Group {
    fn from(ids: Vec<EndpointId>) -> Self {
        let mut hs = HashSet::new();
        hs.extend(ids);

        Self {
            label: None,
            endpoint_ids: hs,
        }
    }
}

/// An endpoint as described by a configuration file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEndpoint {
    /// The path to the endpoint.
    /// Likely "/dev/ttyACMx" or "COMx".
    pub endpoint_id: EndpointId,

    /// An optional label for this endpoint.
    /// See [`Label`].
    pub label: Option<Label>,
}

/// The configuration used for running the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// The endpoints the server should set up when starting.
    pub endpoints: Vec<ConfigEndpoint>,

    /// Logical groupings of endpoints.
    /// See [`Group`].
    pub groups: Vec<Group>,

    /// Should all found serial ports be opened automatically?
    ///
    /// For example on Windows this will open all "COMn" ports found.
    /// On Unix, this will TODO.
    pub auto_open_serial_ports: bool,
}

impl Config {
    fn check_duplicates_across_groups(&self) -> Result<(), Error> {
        let duplicates = self
            .groups
            .iter()
            .flat_map(|group| &group.endpoint_ids)
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
            if group.endpoint_ids.is_empty() {
                return Err(Error::BadConfig(format!("The group with index {index} (zero indexed) is empty. If defining groups, please put entries into it.")));
            }
        }

        Ok(())
    }

    fn check_group_variant_homogeneity(&self) -> Result<(), Error> {
        for (index, group) in self.groups.iter().enumerate() {
            let (mocks, ttys): (Vec<_>, Vec<_>) = group
                .endpoint_ids
                .iter()
                .partition(|id| matches!(id, &EndpointId::Mock(_)));

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
            endpoints: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use ron::{extensions::Extensions, Options};

    use super::*;

    impl Group {
        fn new(endpoints: HashSet<EndpointId>) -> Self {
            Self {
                label: None,
                endpoint_ids: endpoints,
            }
        }

        fn new_with_label(label: &str, endpoints: HashSet<EndpointId>) -> Self {
            Self {
                label: Some(Label::new(label)),
                endpoint_ids: endpoints,
            }
        }
    }

    #[test]
    fn serialize() {
        let mut g1 = HashSet::new();
        g1.extend(vec![
            EndpointId::Tty("COM0".into()),
            EndpointId::Tty("COM1".into()),
            EndpointId::Tty("COM2".into()),
        ]);

        let mut g2 = HashSet::new();
        g2.extend(vec![
            EndpointId::mock("/dev/ttyACM123"),
            EndpointId::mock("some-mock"),
            EndpointId::mock("another-mock"),
        ]);

        let c = Config {
            groups: vec![Group::new(g1), Group::new_with_label("mocks", g2)],
            auto_open_serial_ports: true,
            endpoints: vec![
                ConfigEndpoint {
                    endpoint_id: EndpointId::Tty("COM1".into()),
                    label: Some(Label::new("device-type-1")),
                },
                ConfigEndpoint {
                    endpoint_id: EndpointId::Mock("Mock1".into()),
                    label: None,
                },
            ],
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
    endpoints: [
        (
            endpoint_id: Tty("COM1"),
            label: "device-type-1",
        ),
        (
            endpoint_id: Mock("Mock1"),
            label: None,
        ),
    ],
    groups: [
        (
            label: None,
            endpoint_ids: [
                Tty("COM1"),
                Tty("COM0"),
                Tty("COM2"),
            ],
        ),
        (
            label: "mocks",
            endpoint_ids: [
                Mock("/dev/ttyACM123"),
                Mock("some-mock"),
                Mock("another-mock"),
            ],
        ),
    ],
    auto_open_serial_ports: true,
)
"#;
        let ron = Options::default()
            .with_default_extension(Extensions::IMPLICIT_SOME)
            .with_default_extension(Extensions::UNWRAP_NEWTYPES);
        ron.from_str::<Config>(input).unwrap();
    }

    #[test]
    fn bad_config_duplicates() {
        let mut g1 = HashSet::new();
        g1.extend(vec![
            EndpointId::Tty("COM0".into()),
            EndpointId::Tty("COM1".into()),
            EndpointId::Tty("COM2".into()),
            EndpointId::Tty("COM3".into()),
            EndpointId::Tty("COM4".into()),
            EndpointId::Tty("COM5".into()),
            EndpointId::Tty("COM6".into()),
        ]);

        let mut g2 = HashSet::new();
        g2.extend(vec![
            EndpointId::Tty("COM10".into()),
            EndpointId::Tty("COM11".into()),
            EndpointId::Tty("COM4".into()), // Duplicate!
            EndpointId::Tty("COM5".into()), // Duplicate!
            EndpointId::Tty("COM12".into()),
        ]);

        let c = Config {
            groups: vec![Group::new(g1), Group::new(g2)],
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
        g1.extend(vec![EndpointId::Tty("COM0".into())]);

        let mut g2 = HashSet::new();
        g2.extend(vec![EndpointId::Tty("COM2".into())]);

        let g3 = HashSet::new();

        let c = Config {
            groups: vec![Group::new(g1), Group::new(g2), Group::new(g3)],
            ..Default::default()
        };

        let err = c.validate().unwrap_err().try_into_bad_config().unwrap();

        // Error message countains the index of our bad group
        assert!(err.contains("index 2"));
    }

    #[test]
    fn bad_config_homogeneity() {
        let mut g1 = HashSet::new();
        g1.extend(vec![EndpointId::Tty("COM0".into())]);

        let mut g2 = HashSet::new();
        g2.extend(vec![EndpointId::Mock("Mock0".into())]);

        let mut g3 = HashSet::new();
        g3.extend(vec![
            EndpointId::Mock("Mock1".into()),
            EndpointId::Tty("COM1".into()),
        ]);

        let c = Config {
            groups: vec![Group::new(g1), Group::new(g2), Group::new(g3)],
            ..Default::default()
        };

        let err = c.validate().unwrap_err().try_into_bad_config().unwrap();

        dbg!(&err);

        // Error message mentions the members of the problematic group
        assert!(err.contains("Mock1"));
        assert!(err.contains("COM1"));
    }
}
