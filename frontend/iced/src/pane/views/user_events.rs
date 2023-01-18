use iced::{
    widget::{column, container, scrollable, text},
    Element, Length,
};

use crate::{
    pane::{Events, PaneMessage, TimeDisplaySetting},
    reusable::{container_fill_center, fonts},
};

fn view_empty<'a>() -> Element<'a, PaneMessage> {
    // TODO: Italics font
    container_fill_center(text("No events").size(32))
}

fn view_events<'a>(
    events: Events,
    id: scrollable::Id,
    time_display: TimeDisplaySetting,
    font_size: u16,
) -> Element<'a, PaneMessage> {
    container(
        scrollable(
            column(
                events
                    .iter()
                    .map(|(event, date)| {
                        text(match time_display {
                            TimeDisplaySetting::Absolute => {
                                format!("{}: {event}", date.time().format("%H:%M:%S%.3f"))
                            }
                            // TODO
                            TimeDisplaySetting::Relative => {
                                format!("{}: {event}", date.time().format("%H:%M:%S%.3f"))
                            }
                            TimeDisplaySetting::None => event.to_string(),
                        })
                        .font(fonts::MONO)
                        .size(font_size)
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
    font_size: u16,
) -> Element<'a, PaneMessage> {
    match id {
        Some(id) => view_events(events, id, time_display, font_size),
        None => view_empty(),
    }
}
