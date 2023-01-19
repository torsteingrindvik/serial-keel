use iced::{
    widget::{Column, Container, Radio, Text},
    Element,
};
use iced_aw::tabs::TabBarStyles;

use crate::{Icon, Message, Tab};

// Of the radio buttons themselves
const RADIO_SIZE: u16 = 20;

const RADIO_PADDING: u16 = 10;

const RADIO_TEXT_HEADING_SIZE: u16 = 20;

// Between radio options
const RADIO_VERTICAL_SPACING: u16 = 20;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BarPosition {
    #[default]
    Top,
    Bottom,
}

impl BarPosition {
    const ALL: [BarPosition; 2] = [BarPosition::Top, BarPosition::Bottom];
}

impl From<BarPosition> for String {
    fn from(position: BarPosition) -> Self {
        match position {
            BarPosition::Top => "Top",
            BarPosition::Bottom => "Bottom",
        }
        .into()
    }
}

#[derive(Debug)]
pub struct TabSettings {
    bar_position: BarPosition,
    bar_theme: TabBarStyles,
}

impl Default for TabSettings {
    fn default() -> Self {
        Self {
            bar_position: Default::default(),
            bar_theme: TabBarStyles::Blue,
        }
    }
}

impl TabSettings {
    pub fn bar_position(&self) -> BarPosition {
        self.bar_position
    }

    pub fn bar_theme(&self) -> TabBarStyles {
        self.bar_theme
    }
}

pub struct SettingsTab {
    settings: TabSettings,
}

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    PositionSelected(BarPosition),
    ThemeSelected(TabBarStyles),
}

impl SettingsTab {
    pub fn new() -> Self {
        Self {
            settings: TabSettings::default(),
        }
    }

    pub fn update(&mut self, message: SettingsMessage) {
        match message {
            SettingsMessage::PositionSelected(position) => self.settings.bar_position = position,
            SettingsMessage::ThemeSelected(theme) => self.settings.bar_theme = theme,
        }
    }

    pub fn settings(&self) -> &TabSettings {
        &self.settings
    }
}

impl Tab for SettingsTab {
    type Message = Message;

    fn title(&self) -> String {
        String::from("Settings")
    }

    fn tab_icon(&self) -> Icon {
        Icon::CogAlt
    }

    fn content(&self) -> Element<Message> {
        let content: Element<SettingsMessage> = Container::new(
            Column::new()
                .push(Text::new("Position:").size(RADIO_TEXT_HEADING_SIZE))
                .push(
                    BarPosition::ALL.iter().cloned().fold(
                        Column::new()
                            .padding(RADIO_PADDING)
                            .spacing(RADIO_VERTICAL_SPACING),
                        |column, position| {
                            column.push(
                                Radio::new(
                                    position,
                                    position,
                                    Some(self.settings().bar_position()),
                                    SettingsMessage::PositionSelected,
                                )
                                .size(RADIO_SIZE),
                            )
                        },
                    ),
                )
                .push(Text::new("Theme:").size(RADIO_TEXT_HEADING_SIZE))
                .push(
                    (0..5).fold(
                        Column::new()
                            .padding(RADIO_PADDING)
                            .spacing(RADIO_VERTICAL_SPACING),
                        |column, id| {
                            column.push(
                                Radio::new(
                                    style(id),
                                    style(id),
                                    Some(self.settings().bar_theme()),
                                    SettingsMessage::ThemeSelected,
                                )
                                .size(RADIO_SIZE),
                            )
                        },
                    ),
                ),
        )
        .into();

        content.map(Message::Settings)
    }
}

fn style(index: usize) -> TabBarStyles {
    match index {
        0 => TabBarStyles::Default,
        1 => TabBarStyles::Red,
        2 => TabBarStyles::Blue,
        3 => TabBarStyles::Green,
        4 => TabBarStyles::Purple,
        _ => TabBarStyles::Default,
    }
}
