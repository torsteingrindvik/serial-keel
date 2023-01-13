use iced::{widget::container, Element, Length};

pub fn container_fill_center<'a, T: 'a>(content: impl Into<Element<'a, T>>) -> Element<'a, T> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x()
        .center_y()
        .into()
}
