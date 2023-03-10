use std::path::Path;

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::{
    endpoint::{EndpointId, Label, Labels},
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
    /// The label(s) for the group.
    /// Will propagate to each endpoint.
    pub labels: Labels,

    /// The endpoints which are members of this group.
    /// Implies shared control.
    pub endpoints: Vec<ConfigEndpoint>,
}

impl From<EndpointId> for ConfigEndpoint {
    fn from(endpoint_id: EndpointId) -> Self {
        Self {
            id: endpoint_id,
            labels: Labels::default(),
        }
    }
}

impl Group {
    fn new(endpoints: Vec<EndpointId>) -> Self {
        Self {
            labels: Labels::default(),
            endpoints: endpoints.into_iter().map(Into::into).collect(),
        }
    }
    pub(crate) fn is_mock_group(&self) -> bool {
        let member = self.endpoints.iter().last().expect("Groups are non-empty");

        matches!(member.id, EndpointId::Mock(_))
    }

    /// Make a group where the group itself has some label.
    pub fn new_with_labels<S: AsRef<str>>(labels: &[S], endpoints: Vec<EndpointId>) -> Self {
        Self {
            labels: Labels::from_iter(labels),
            endpoints: endpoints.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<Vec<EndpointId>> for Group {
    fn from(ids: Vec<EndpointId>) -> Self {
        Self::new(ids)
    }
}

/// An endpoint as described by a configuration file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEndpoint {
    /// The path to the endpoint.
    /// Likely "/dev/ttyACMx" or "COMx".
    pub id: EndpointId,

    /// An optional label for this endpoint.
    /// See [`Label`].
    pub labels: Labels,
}

/// The configuration used for running the server.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// The endpoints the server should set up when starting.
    // TODO: Use Option<..> to allow omitting in config
    pub endpoints: Vec<ConfigEndpoint>,

    /// Logical groupings of endpoints.
    /// See [`Group`].
    // TODO: Use Option<..> to allow omitting in config
    pub groups: Vec<Group>,
}

impl Config {
    fn ron() -> ron::Options {
        ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .with_default_extension(ron::extensions::Extensions::UNWRAP_NEWTYPES)
    }

    /// Deserialize a .ron file's contents.
    /// Panics if the input is not valid .ron.
    pub fn deserialize(input: &str) -> Self {
        Self::ron().from_str::<Config>(input).unwrap()
    }

    /// An example configuration with some fields filled in.
    pub fn example() -> Self {
        let g1 = vec![
            EndpointId::Tty("COM0".into()),
            EndpointId::Tty("COM1".into()),
            EndpointId::Tty("COM2".into()),
        ];

        let g2 = vec![
            EndpointId::mock("/dev/ttyMock"),
            EndpointId::mock("some-mock"),
            EndpointId::mock("another-mock"),
        ];

        Self {
            groups: vec![Group::new(g1), Group::new_with_labels(&["mocks"], g2)],
            endpoints: vec![
                ConfigEndpoint {
                    id: EndpointId::Tty("COM1".into()),
                    labels: Labels::from_iter([Label::new("device-type-1")]),
                },
                ConfigEndpoint {
                    id: EndpointId::Mock("Mock1".into()),
                    labels: Labels::default(),
                },
            ],
        }
    }

    /// Serialize the configuration in a "pretty" (i.e. non-compact) fashion.
    pub fn serialize_pretty(&self) -> String {
        Self::ron()
            .to_string_pretty(self, ron::ser::PrettyConfig::default())
            .unwrap()
    }

    /// Setup a new configuration from a RON file.
    pub fn new_from_path<P: AsRef<Path>>(p: P) -> Self {
        let s = std::fs::read_to_string(p).unwrap();

        Self::deserialize(&s)
    }

    fn check_duplicates_across_groups(&self) -> Result<(), Error> {
        let duplicates = self
            .groups
            .iter()
            .flat_map(|group| &group.endpoints)
            .map(|ce| &ce.id)
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
            if group.endpoints.is_empty() {
                return Err(Error::BadConfig(format!("The group with index {index} (zero indexed) is empty. If defining groups, please put entries into it.")));
            }
        }

        Ok(())
    }

    fn check_group_variant_homogeneity(&self) -> Result<(), Error> {
        for (index, group) in self.groups.iter().enumerate() {
            let (mocks, ttys): (Vec<_>, Vec<_>) = group
                .endpoints
                .iter()
                .partition(|id| matches!(&id.id, &EndpointId::Mock(_)));

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize() {
        let c = Config::example();

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
            id: Tty("COM1"),    
            labels: [
                "device-type-1",
            ],
        ),
        (
            id: Mock("Mock1"),  
            labels: [],
        ),
    ],
    groups: [
        (
            labels: [],
            endpoints: [
                (
                    id: Tty("COM0"),
                    labels: [],
                ),
                (
                    id: Tty("COM1"),
                    labels: [],
                ),
                (
                    id: Tty("COM2"),
                    labels: [],
                ),
            ],
        ),
        (
            labels: ["mocks"],
            endpoints: [
                (
                    id: Mock("/dev/ttyMock"),
                    labels: [],
                ),
                (
                    id: Mock("some-mock"),
                    labels: [],
                ),
                (
                    id: Mock("another-mock"),
                    labels: [],
                ),
            ],
        ),
    ],
    auto_open_serial_ports: true,
)
"#;
        let _config = Config::deserialize(input);
    }

    #[test]
    fn bad_config_duplicates() {
        let g1 = vec![
            EndpointId::Tty("COM0".into()),
            EndpointId::Tty("COM1".into()),
            EndpointId::Tty("COM2".into()),
            EndpointId::Tty("COM3".into()),
            EndpointId::Tty("COM4".into()),
            EndpointId::Tty("COM5".into()),
            EndpointId::Tty("COM6".into()),
        ];

        let g2 = vec![
            EndpointId::Tty("COM10".into()),
            EndpointId::Tty("COM11".into()),
            EndpointId::Tty("COM4".into()), // Duplicate!
            EndpointId::Tty("COM5".into()), // Duplicate!
            EndpointId::Tty("COM12".into()),
        ];

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
        let g1 = vec![EndpointId::Tty("COM0".into())];

        let g2 = vec![EndpointId::Tty("COM2".into())];

        let g3 = vec![];

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
        let g1 = vec![EndpointId::Tty("COM0".into())];
        let g2 = vec![EndpointId::Mock("Mock0".into())];
        let g3 = vec![
            EndpointId::Mock("Mock1".into()),
            EndpointId::Tty("COM1".into()),
        ];

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
