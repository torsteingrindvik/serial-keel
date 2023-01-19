use iced::{widget::container, Element, Length};

pub mod fonts {
    use iced::Font;

    pub const ICONS: Font = Font::External {
        name: "Icons",
        // bytes: include_bytes!("../assets/fonts/icons/icons.ttf"),
        // bytes: include_bytes!("../assets/fonts/icons/fa-regular-400.ttf"),
        bytes: include_bytes!("../assets/fonts/icons/bootstrap-icons.ttf"),
    };

    pub const MONO: Font = Font::External {
        name: "FiraMono",
        bytes: include_bytes!("../assets/fonts/mono/FiraMono-Medium.ttf"),
    };
}

pub enum Icon {
    User,
    Heart,
    Calc,
    CogAlt,
    Server,
}

impl From<Icon> for char {
    fn from(icon: Icon) -> Self {
        match icon {
            // See https://fontawesome.com/icons
            Icon::User => '\u{E800}',
            Icon::Heart => '\u{f267}',
            Icon::Calc => '\u{F1EC}',
            Icon::CogAlt => '\u{f3e5}',
            // Icon::Server => '\u{1f48e}',
            // Icon::Server => '\u{1f441}',
            Icon::Server => '\u{f40f}',
        }
    }
}

pub fn container_fill_center<'a, T: 'a>(content: impl Into<Element<'a, T>>) -> Element<'a, T> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x()
        .center_y()
        .into()
}
