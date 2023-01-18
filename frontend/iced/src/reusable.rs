use iced::{widget::container, Element, Length};

pub mod fonts {
    use iced::Font;

    pub const ICONS: Font = Font::External {
        name: "Icons",
        bytes: include_bytes!("../assets/fonts/icons/icons.ttf"),
    };

    pub const MONO: Font = Font::External {
        name: "FiraMono",
        bytes: include_bytes!("../assets/fonts/mono/FiraMono-Medium.ttf"),
    };
}

pub fn container_fill_center<'a, T: 'a>(content: impl Into<Element<'a, T>>) -> Element<'a, T> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x()
        .center_y()
        .into()
}
