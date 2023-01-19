use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
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
    Color, Command, Element, Length,
};
use iced_aw::{style::SelectionListStyles, SelectionList};
use serial_keel::{client::*, user::User};

use crate::{
    reusable::{container_fill_center, fonts},
    Icon, Message, Tab,
};

#[derive(Debug)]
enum PaneVariant {
    Users,
    UserEvents,
}

#[derive(Debug)]
struct UserEventState {
    events: BTreeMap<User, Vec<(Event, DateTime<Utc>)>>,
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

type Events = Vec<(Event, DateTime<Utc>)>;

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

#[derive(Debug)]
struct PaneState {
    variant: PaneVariant,
    user_events_state: SharedState,
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

impl PaneState {
    fn state(&self) -> RwLockReadGuard<UserEventState> {
        self.user_events_state
            .try_read()
            .expect("Should be able to read state")
    }

    fn view_empty<'a>(&self) -> Element<'a, PaneMessage> {
        // TODO: Italics font
        container_fill_center(text("No users").size(32))
    }

    fn view_users<'a>(&self, users: Vec<UserAndNumEvents>) -> Element<'a, PaneMessage> {
        container_fill_center(
            SelectionList::new_with(
                users,
                |user_and_num_events| PaneMessage::UserSelected(user_and_num_events.user),
                25,
                15,
                SelectionListStyles::Default,
            )
            .width(Length::Fill),
        )
    }

    fn view_user_list<'a>(&'a self) -> Element<'a, PaneMessage> {
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

    fn view_user_events<'a>(&'a self) -> Element<'a, PaneMessage> {
        let state = self.state();

        let Some((user, id)) = state.selected_user.as_ref().map(|user| {
            (user, state
                .scroll_ids
                .get(&user)
                .expect("User should have a scroll id")
                .clone())
        }) else {
            return container_fill_center(text("No events").size(32));
        };

        let events = state.events();
        let time_display = state.time_display_setting;
        let font_size = state.font_size;
        let first = state
            .first_event
            .get(&user)
            .expect("User should have a first event")
            .time();

        container(
            widget::scrollable(
                widget::column(
                    events
                        .iter()
                        .map(|(event, date)| {
                            text(match time_display {
                                TimeDisplaySetting::Absolute => {
                                    format!("{}: {event}", date.time().format("%H:%M:%S%.3f"))
                                }
                                TimeDisplaySetting::Relative => {
                                    let diff = date.time() - first;
                                    let secs = diff.to_std().unwrap().as_secs_f32();

                                    format!("{secs:10.3}: {event}")
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

    fn view_content<'a>(&'a self) -> Element<'a, PaneMessage> {
        match self.variant {
            PaneVariant::Users => self.view_user_list(),
            PaneVariant::UserEvents => self.view_user_events(),
        }
    }
}

impl PaneState {
    fn new(variant: PaneVariant, events: Arc<RwLock<UserEventState>>) -> Self {
        Self {
            variant,
            user_events_state: events,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PaneMessage {
    Clicked(pane_grid::Pane),
    Resized(pane_grid::ResizeEvent),
    TimeDisplaySettingChanged(TimeDisplaySetting),
    UserSelected(User),
    UserEvent(UserEvent),
    FontSizeChanged(u16),
}

impl From<UserEvent> for PaneMessage {
    fn from(v: UserEvent) -> Self {
        Self::UserEvent(v)
    }
}

type SharedState = Arc<RwLock<UserEventState>>;

pub struct PaneTab {
    shared_state: SharedState,
    panes: pane_grid::State<PaneState>,
    focus: Option<pane_grid::Pane>,
}

impl PaneTab {
    pub fn new() -> Self {
        let shared_state: SharedState = Default::default();

        let (mut panes, pane) =
            pane_grid::State::new(PaneState::new(PaneVariant::Users, shared_state.clone()));
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

    fn state_mut(&mut self) -> RwLockWriteGuard<'_, UserEventState> {
        self.shared_state
            .try_write()
            .expect("Should not write state while viewing")
    }

    fn state(&self) -> RwLockReadGuard<UserEventState> {
        self.shared_state
            .try_read()
            .expect("Should be able to read state")
    }

    pub fn update(&mut self, message: PaneMessage) -> Command<PaneMessage> {
        match message {
            PaneMessage::Clicked(pane) => {
                self.focus = Some(pane);
            }
            PaneMessage::Resized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(&split, ratio);
            }
            PaneMessage::UserSelected(user) => {
                let mut state = self.state_mut();
                state.selected_user = Some(user);
            }
            PaneMessage::UserEvent(user_event) => {
                let mut state = self.state_mut();

                let user = user_event.user;

                let id = state
                    .scroll_ids
                    .entry(user.clone())
                    .or_insert_with(|| scrollable::Id::unique())
                    .clone();

                let user_events = state.events.entry(user.clone()).or_default();
                let was_empty = user_events.is_empty();
                user_events.push((user_event.event, user_event.timestamp));

                if was_empty {
                    state.first_event.insert(user, user_event.timestamp);
                }

                return snap_to(id, RelativeOffset::END);
            }
            PaneMessage::TimeDisplaySettingChanged(to) => {
                self.state_mut().time_display_setting = to;
            }
            PaneMessage::FontSizeChanged(size) => {
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

impl Tab for PaneTab {
    type Message = Message;

    fn title(&self) -> String {
        String::from("User Events")
    }

    fn tab_icon(&self) -> crate::Icon {
        Icon::Heart
    }

    fn content(&self) -> Element<Message> {
        let pane_grid: Element<PaneMessage> =
            PaneGrid::new(&self.panes, |pane, state, _is_maximized| {
                let is_focused = self.focus == Some(pane);

                // let title = row![
                //     "Pane",
                //     text(pane.id.to_string()).style(if is_focused {
                //         PANE_ID_COLOR_FOCUSED
                //     } else {
                //         PANE_ID_COLOR_UNFOCUSED
                //     }),
                // ];

                // let title_bar = pane_grid::TitleBar::new(title).padding(5);

                pane_grid::Content::new(state.view_content())
                    // .title_bar(title_bar)
                    .style(if is_focused {
                        style::pane_focused
                    } else {
                        style::pane_active
                    })
            })
            .spacing(5)
            .width(Length::Fill)
            .height(Length::Fill)
            .on_click(PaneMessage::Clicked)
            .on_resize(10, PaneMessage::Resized)
            .into();

        let time_settings: Element<PaneMessage> = TimeDisplaySetting::ALL
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
                            PaneMessage::TimeDisplaySettingChanged,
                        )
                        .size(10),
                    )
                },
            )
            .into();

        let time_settings: Element<PaneMessage> = container(time_settings)
            .height(Length::Shrink)
            .center_y()
            .into();

        let font_settings: Element<PaneMessage> = widget::column![
            text("Font size"),
            widget::slider(8..=40, self.state().font_size, |v| {
                PaneMessage::FontSizeChanged(v as u16)
            })
            .width(Length::Units(200))
        ]
        .padding(5)
        .spacing(5)
        .into();

        let font_settings: Element<PaneMessage> = container(font_settings)
            .width(Length::Fill)
            .height(Length::Shrink)
            .center_y()
            .into();

        let contents = widget::column![
            row![
                time_settings.map(Message::Pane),
                font_settings.map(Message::Pane),
            ]
            .width(Length::Fill)
            .spacing(50)
            .padding(10),
            pane_grid.map(Message::Pane)
        ];

        container(contents)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

// fn view_content<'a>(state: &PaneState) -> Element<'a, PaneMessage> {
//     dbg!(state);
//     let some_buttons = row![Button::new(text("Hey"))];

//     let users = column(
//         (0..100)
//             .into_iter()
//             .map(|i| Text::new(format!("User #{}", i)).into())
//             .collect(),
//     )
//     .width(Length::Fill);

//     let contents = column![some_buttons, users];

//     container(scrollable(contents))
//         .padding(5)
//         .width(Length::Fill)
//         .height(Length::Fill)
//         .into()
// }

const PANE_ID_COLOR_UNFOCUSED: Color = Color::from_rgb(
    0xFF as f32 / 255.0,
    0xC7 as f32 / 255.0,
    0xC7 as f32 / 255.0,
);
const PANE_ID_COLOR_FOCUSED: Color = Color::from_rgb(
    0xFF as f32 / 255.0,
    0x47 as f32 / 255.0,
    0x47 as f32 / 255.0,
);

mod style {
    use iced::widget::container;
    use iced::Theme;

    pub fn title_bar_active(theme: &Theme) -> container::Appearance {
        let palette = theme.extended_palette();

        container::Appearance {
            text_color: Some(palette.background.strong.text),
            background: Some(palette.background.strong.color.into()),
            ..Default::default()
        }
    }

    pub fn title_bar_focused(theme: &Theme) -> container::Appearance {
        let palette = theme.extended_palette();

        container::Appearance {
            text_color: Some(palette.primary.strong.text),
            background: Some(palette.primary.strong.color.into()),
            ..Default::default()
        }
    }

    pub fn pane_active(theme: &Theme) -> container::Appearance {
        let palette = theme.extended_palette();

        container::Appearance {
            background: Some(palette.background.weak.color.into()),
            border_width: 2.0,
            border_color: palette.background.strong.color,
            ..Default::default()
        }
    }

    pub fn pane_focused(theme: &Theme) -> container::Appearance {
        let palette = theme.extended_palette();

        container::Appearance {
            background: Some(palette.background.weak.color.into()),
            border_width: 2.0,
            border_color: palette.primary.strong.color,
            ..Default::default()
        }
    }
}
