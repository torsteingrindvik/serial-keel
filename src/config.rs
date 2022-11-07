use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::endpoint::EndpointLabel;

#[derive(Debug, Serialize, Deserialize)]
pub struct Group(pub(crate) HashSet<EndpointLabel>);

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Config {
    pub(crate) groups: Vec<Group>,
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
        };

        println!(
            "{}",
            ron::ser::to_string_pretty(&c, ron::ser::PrettyConfig::default()).unwrap()
        );
    }

    #[test]
    fn deserialize() {
        let input = r#"(groups:[([Tty("COM1"),Tty("COM0"),Tty("COM2")]),([Mock("another-mock"),Mock("some-mock"),Mock("/dev/ttyACM123")])])"#;
        ron::from_str::<Config>(input).unwrap();

        let input = r#"
(
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
}
