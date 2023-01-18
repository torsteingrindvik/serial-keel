use iced::{
    widget::{column, container, row, scrollable::Properties, Scrollable, Text},
    Element, Length,
};

use crate::{Icon, Message, Tab};

pub struct ScrollableTab {}

impl ScrollableTab {
    pub fn new() -> Self {
        Self {}
    }

    pub fn update(&mut self, message: ScrollableMessage) {
        match message {
            ScrollableMessage::Click => {}
        }
    }
}

#[derive(Debug, Clone)]
pub enum ScrollableMessage {
    Click,
}

impl Tab for ScrollableTab {
    type Message = Message;

    fn title(&self) -> String {
        String::from("User Events")
    }

    fn tab_icon(&self) -> crate::Icon {
        Icon::Calc
    }

    fn content(&self) -> Element<Message> {
        let users = column(
            (0..100)
                .into_iter()
                .map(|i| Text::new(format!("User #{}", i)).into())
                .collect(),
        )
        .width(Length::Units(200));

        let user_events = column(
            (0..100)
                .into_iter()
                .map(|i| Text::new(format!("User Event #{}", i)).into())
                .collect(),
        )
        .width(Length::Fill);

        let content = row![
            Scrollable::new(users)
                .vertical_scroll(Properties::new().width(4).margin(3).scroller_width(4)),
            Scrollable::new(user_events)
                .vertical_scroll(Properties::new().width(4).margin(3).scroller_width(4)),
        ]
        .spacing(20)
        .padding(20);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
