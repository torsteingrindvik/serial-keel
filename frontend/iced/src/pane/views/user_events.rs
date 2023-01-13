use iced::{
    widget::{column, container, scrollable, text},
    Element, Length,
};

use crate::{
    pane::{Events, PaneMessage, TimeDisplaySetting},
    reusable::container_fill_center,
};

fn view_empty<'a>() -> Element<'a, PaneMessage> {
    // TODO: Italics font
    container_fill_center(text("No events").size(32))
}

fn view_events<'a>(
    events: Events,
    id: scrollable::Id,
    time_display: TimeDisplaySetting,
) -> Element<'a, PaneMessage> {
    container(
        scrollable(
            column(
                events
                    .iter()
                    // .map(|(event, date)| text(format!("{}: {}", date, event)).into())
                    .map(|(event, date)| {
                        text(match time_display {
                            TimeDisplaySetting::Absolute => format!("{date}: {event}"),
                            TimeDisplaySetting::Relative => format!("<relative>: {event}"),
                            TimeDisplaySetting::None => event.to_string(),
                        })
                        .into()
                    })
                    .collect(),
            )
            .width(Length::Fill),
        )
        .id(id),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(5)
    .into()
}

pub fn view<'a>(
    events: Events,
    id: Option<scrollable::Id>,
    time_display: TimeDisplaySetting,
) -> Element<'a, PaneMessage> {
    match id {
        Some(id) => view_events(events, id, time_display),
        None => view_empty(),
    }
}
