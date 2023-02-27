use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::{RwLockReadGuard, RwLockWriteGuard},
    vec,
};

use iced::{
    widget::{
        self, container,
        pane_grid::{self, Axis},
        row,
        scrollable::{self, snap_to, RelativeOffset},
        text, PaneGrid, Radio,
    },
    Command, Element, Length,
};
use iced_aw::{style::SelectionListStyles, SelectionList};
use serial_keel::{client::*, events::user, user::User};

use crate::{
    reusable::{self, containers, elements, fonts},
    Icon, Message, Tab,
};

type SharedState = reusable::state::SharedState<UserEventState>;

#[derive(Debug)]
enum PaneVariant {
    UsersList,
    UserEvents,
}

#[derive(Debug)]
struct UserEventState {
    events: BTreeMap<User, Vec<(user::Event, DateTime<Utc>)>>,
    scroll_ids: HashMap<User, scrollable::Id>,
    first_event: HashMap<User, DateTime<Utc>>,
    selected_user: Option<User>,
    time_display_setting: TimeDisplaySetting,
    font_size: u16,
}

impl Default for UserEventState {
    fn default() -> Self {
        Self {
            events: Default::default(),
            scroll_ids: Default::default(),
            first_event: Default::default(),
            selected_user: Default::default(),
            time_display_setting: Default::default(),
            font_size: 18,
        }
    }
}

type Events = Vec<(user::Event, DateTime<Utc>)>;

impl UserEventState {
    fn users(&self) -> Vec<User> {
        self.events.keys().cloned().collect()
    }

    fn num_user_events(&self, user: &User) -> usize {
        self.events
            .get(user)
            .map(|events| events.len())
            .unwrap_or(0)
    }

    fn events(&self) -> Events {
        match &self.selected_user {
            Some(user) => match self.events.get(user) {
                Some(events) => events.iter().rev().take(100).rev().cloned().collect(),
                None => vec![],
            },
            None => vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UserAndNumEvents {
    user: User,
    num_events: usize,
}

impl fmt::Display for UserAndNumEvents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.user, self.num_events)
    }
}

#[derive(Debug)]
struct PaneState {
    variant: PaneVariant,
    user_events_state: SharedState,
}

impl PaneState {
    fn new(variant: PaneVariant, events: SharedState) -> Self {
        Self {
            variant,
            user_events_state: events,
        }
    }

    fn state(&self) -> RwLockReadGuard<UserEventState> {
        self.user_events_state.state()
    }

    fn view_empty<'a>(&self) -> Element<'a, UserEventsTabMessage> {
        elements::empty("No users")
    }

    fn view_users<'a>(&self, users: Vec<UserAndNumEvents>) -> Element<'a, UserEventsTabMessage> {
        containers::fill_centered_xy(
            SelectionList::new_with(
                users,
                |user_and_num_events| UserEventsTabMessage::UserSelected(user_and_num_events.user),
                25,
                15,
                SelectionListStyles::Default,
            )
            .width(Length::Fill),
        )
    }

    fn view_user_list(&self) -> Element<UserEventsTabMessage> {
        let state = self.state();
        let users = state.users();
        if users.is_empty() {
            self.view_empty()
        } else {
            self.view_users(
                users
                    .into_iter()
                    .map(|user| UserAndNumEvents {
                        num_events: state.num_user_events(&user),
                        user,
                    })
                    .collect(),
            )
        }
    }

    fn view_user_events(&self) -> Element<UserEventsTabMessage> {
        let state = self.state();

        let Some((user, id)) = state.selected_user.as_ref().map(|user| {
            (user, state
                .scroll_ids
                .get(user)
                .expect("User should have a scroll id")
                .clone())
        }) else {
            return elements::empty("No events");
        };

        let events = state.events();
        let time_display = state.time_display_setting;
        let font_size = state.font_size;
        let first = state
            .first_event
            .get(user)
            .expect("User should have a first event");

        container(
            widget::scrollable(
                widget::column(
                    events
                        .iter()
                        .map(|(event, date_time)| {
                            text(match time_display {
                                TimeDisplaySetting::Absolute => {
                                    format!(
                                        "{}: {event}",
                                        reusable::time::human_readable_absolute(date_time)
                                    )
                                }
                                TimeDisplaySetting::Relative => {
                                    format!(
                                        "{}: {event}",
                                        reusable::time::human_readable_relative(first, date_time)
                                    )
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

    fn view_content(&self) -> Element<UserEventsTabMessage> {
        match self.variant {
            PaneVariant::UsersList => self.view_user_list(),
            PaneVariant::UserEvents => self.view_user_events(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum UserEventsTabMessage {
    Clicked(pane_grid::Pane),
    Resized(pane_grid::ResizeEvent),
    TimeDisplaySettingChanged(TimeDisplaySetting),
    UserSelected(User),
    UserEvent((user::UserEvent, DateTime<Utc>)),
    FontSizeChanged(u16),
}

// impl From<UserEvent> for UserEventsTabMessage {
//     fn from(v: UserEvent) -> Self {
//         Self::UserEvent(v)
//     }
// }

pub struct UserEventsTab {
    shared_state: SharedState,
    panes: pane_grid::State<PaneState>,
    focus: Option<pane_grid::Pane>,
}

impl UserEventsTab {
    pub fn new() -> Self {
        let shared_state: SharedState = Default::default();

        let (mut panes, pane) =
            pane_grid::State::new(PaneState::new(PaneVariant::UsersList, shared_state.clone()));
        let (_, split) = panes
            .split(
                Axis::Vertical,
                &pane,
                PaneState::new(PaneVariant::UserEvents, shared_state.clone()),
            )
            .unwrap();
        panes.resize(&split, 0.25);

        Self {
            focus: None,
            panes,
            shared_state,
        }
    }

    fn state_mut(&mut self) -> RwLockWriteGuard<UserEventState> {
        self.shared_state.state_mut()
    }

    fn state(&self) -> RwLockReadGuard<UserEventState> {
        self.shared_state.state()
    }

    pub fn update(&mut self, message: UserEventsTabMessage) -> Command<UserEventsTabMessage> {
        match message {
            UserEventsTabMessage::Clicked(pane) => {
                self.focus = Some(pane);
            }
            UserEventsTabMessage::Resized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(&split, ratio);
            }
            UserEventsTabMessage::UserSelected(user) => {
                let mut state = self.state_mut();
                state.selected_user = Some(user);
            }
            UserEventsTabMessage::UserEvent((user_event, timestamp)) => {
                let mut state = self.state_mut();

                let user = user_event.user;

                let id = state
                    .scroll_ids
                    .entry(user.clone())
                    .or_insert_with(scrollable::Id::unique)
                    .clone();

                let user_events = state.events.entry(user.clone()).or_default();
                let was_empty = user_events.is_empty();
                user_events.push((user_event.event, timestamp));

                if was_empty {
                    state.first_event.insert(user, timestamp);
                }

                return snap_to(id, RelativeOffset::END);
            }
            UserEventsTabMessage::TimeDisplaySettingChanged(to) => {
                self.state_mut().time_display_setting = to;
            }
            UserEventsTabMessage::FontSizeChanged(size) => {
                self.state_mut().font_size = size;
            }
        }

        Command::none()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TimeDisplaySetting {
    #[default]
    Absolute,
    Relative,
    None,
}

impl TimeDisplaySetting {
    const ALL: [TimeDisplaySetting; 3] = [
        TimeDisplaySetting::Absolute,
        TimeDisplaySetting::Relative,
        TimeDisplaySetting::None,
    ];
}

impl From<TimeDisplaySetting> for String {
    fn from(setting: TimeDisplaySetting) -> Self {
        match setting {
            TimeDisplaySetting::Absolute => "Absolute".to_string(),
            TimeDisplaySetting::Relative => "Relative".to_string(),
            TimeDisplaySetting::None => "None".to_string(),
        }
    }
}

impl Tab for UserEventsTab {
    type Message = Message;

    fn title(&self) -> String {
        String::from("User Events")
    }

    fn tab_icon(&self) -> crate::Icon {
        Icon::Heart
    }

    fn content(&self) -> Element<Message> {
        let pane_grid: Element<UserEventsTabMessage> =
            PaneGrid::new(&self.panes, |pane, state, _is_maximized| {
                let is_focused = self.focus == Some(pane);

                pane_grid::Content::new(state.view_content()).style(if is_focused {
                    reusable::style::pane_focused
                } else {
                    reusable::style::pane_active
                })
            })
            .spacing(5)
            .width(Length::Fill)
            .height(Length::Fill)
            .on_click(UserEventsTabMessage::Clicked)
            .on_resize(10, UserEventsTabMessage::Resized)
            .into();

        let time_settings: Element<UserEventsTabMessage> = TimeDisplaySetting::ALL
            .iter()
            .cloned()
            .fold(
                widget::column![text("Time display")].padding(5).spacing(5),
                |column, setting| {
                    column.push(
                        Radio::new(
                            setting,
                            setting,
                            Some(self.state().time_display_setting),
                            UserEventsTabMessage::TimeDisplaySettingChanged,
                        )
                        .size(10),
                    )
                },
            )
            .into();

        let time_settings: Element<UserEventsTabMessage> = container(time_settings)
            .height(Length::Shrink)
            .center_y()
            .into();

        let font_settings: Element<UserEventsTabMessage> = widget::column![
            text("Font size"),
            widget::slider(8..=40, self.state().font_size, |size| {
                UserEventsTabMessage::FontSizeChanged(size)
            })
            .width(Length::Units(200))
        ]
        .padding(5)
        .spacing(5)
        .into();

        let font_settings: Element<UserEventsTabMessage> = container(font_settings)
            .width(Length::Fill)
            .height(Length::Shrink)
            .center_y()
            .into();

        let contents = widget::column![
            row![
                time_settings.map(Message::UserEventsTab),
                font_settings.map(Message::UserEventsTab),
            ]
            .width(Length::Fill)
            .spacing(50)
            .padding(10),
            pane_grid.map(Message::UserEventsTab)
        ];

        container(contents)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
