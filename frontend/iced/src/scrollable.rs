use iced::{
    widget::{column, container, row, scrollable, Text},
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
        // let content = Container::new(
        //     Column::new()
        //         .width(Length::FillPortion(1))
        //         .push(Text::new("Hi")),
        // )
        // .width(Length::Fill);

        // let content = Column::new()
        //     .width(Length::Fill)
        //     .push(Text::new("Hey"))
        //     .push(vertical_space(Length::Units(100)))
        //     .push(Text::new("Hey 2"))
        //     .push(vertical_space(Length::Units(1000)))
        //     .push(Text::new("Hey 3"))
        //     .push(vertical_space(Length::Units(1000)))
        //     .push(Text::new("Hey 4"))
        //     .push(vertical_space(Length::Units(1000)))
        //     .push(Text::new("Hey 5"))
        //     .push(vertical_space(Length::Units(1000)));

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
            scrollable(users)
                .scrollbar_width(4)
                .scrollbar_margin(3)
                .scroller_width(4),
            scrollable(user_events)
                .scrollbar_width(4)
                .scrollbar_margin(3)
                .scroller_width(4),
        ]
        .spacing(20)
        .padding(20);

        // let mut content: Element<ScrollableMessage> = Container::new(scrollable(content)).into();

        // content.map(Message::Scrollable).into()
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
