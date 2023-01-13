use iced::{widget::text, Element, Length};
use iced_aw::{style::SelectionListStyles, SelectionList};
use serial_keel::user::User;

use crate::{pane::PaneMessage, reusable::container_fill_center};

fn view_empty<'a>() -> Element<'a, PaneMessage> {
    // TODO: Italics font
    container_fill_center(text("No users").size(32))
}

fn view_users<'a>(users: Vec<User>) -> Element<'a, PaneMessage> {
    container_fill_center(
        SelectionList::new_with(
            users,
            PaneMessage::UserSelected,
            25,
            15,
            SelectionListStyles::Default,
        )
        .width(Length::Fill),
    )
}

pub fn view<'a>(users: Vec<User>) -> Element<'a, PaneMessage> {
    if users.is_empty() {
        view_empty()
    } else {
        view_users(users)
    }
}
