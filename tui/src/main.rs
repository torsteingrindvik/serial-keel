use chrono::{DateTime, Utc};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use enum_iterator::Sequence;
use serial_keel::{
    client::{UserEvent, UserEventReader},
    endpoint::EndpointId,
    user::User,
};
use std::{
    collections::HashMap,
    error::Error,
    fmt::Display,
    io,
    time::{Duration, Instant},
};
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::*,
    Frame, Terminal,
};

#[derive(Debug, Copy, Clone, PartialEq, Sequence)]
enum Tab {
    Serial,
    Users,
    Server,
}

impl Tab {
    fn index(&self) -> usize {
        match self {
            Tab::Serial => 0,
            Tab::Users => 1,
            Tab::Server => 2,
        }
    }
}

fn ui_example<B: Backend>(f: &mut Frame<B>, area: Rect, color: Color) {
    let w = Sparkline::default()
        .block(Block::default().title("Sparkline").borders(Borders::ALL))
        .data(&[
            0, 2, 3, 4, 1, 4, 10, 0, 2, 3, 4, 1, 4, 10, 0, 2, 3, 4, 1, 4, 10, 0, 2, 3, 4, 1, 4, 10,
        ])
        .max(5)
        .style(Style::default().fg(color).bg(Color::White));

    f.render_widget(w, area);
}

// fn extend_set(set: &mut BTreeSet<ActiveInactive<EndpointId>>, with: Vec<InternalEndpointInfo>) {
//     set.extend(with.into_iter().map(|e| e.id.into()));
// }

// fn shrink_then_move(
//     active: &mut BTreeSet<ActiveInactive<EndpointId>>,
//     inactive: &mut BTreeSet<ActiveInactive<EndpointId>>,
//     no_longer_active: Vec<InternalEndpointInfo>,
// ) {
//     // extend_set(inactive, no_longer_active);
//     *active = active.difference(inactive).cloned().collect();
// }

// fn set_to_list_item(set: &BTreeSet<EndpointId>, active: bool) -> Vec<ListItem> {
//     let style = if active {
//         Style::default().fg(Color::Green)
//     } else {
//         Style::default().fg(Color::Red)
//     };

//     set.iter()
//         .map(|e| ListItem::new(format!("{}", e)).style(style))
//         .collect()
// }

// fn info_list<'i>(
//     name: &'static str,
//     active: &'i BTreeSet<EndpointId>,
//     inactive: &'i BTreeSet<EndpointId>,
// ) -> impl Widget + 'i {
//     let mut list_items = set_to_list_item(active, true);
//     list_items.extend(set_to_list_item(inactive, false));

//     List::new(list_items).block(Block::default().title(name).borders(Borders::ALL))
// }

#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Timestamp(DateTime<Utc>);

impl From<DateTime<Utc>> for Timestamp {
    fn from(t: DateTime<Utc>) -> Self {
        Self::new(t)
    }
}

impl Timestamp {
    fn new(t: DateTime<Utc>) -> Self {
        Self(t)
    }

    fn now() -> Self {
        Self(chrono::Utc::now())
    }

    /// How much newer is the other timestamp in milliseconds?
    fn ms_difference(&self, other: Timestamp) -> i64 {
        (other.0 - self.0).num_milliseconds()
    }

    fn span_relative_to_ms(&self, other: Timestamp) -> Span {
        let ms = self.ms_difference(other);

        Span::styled(format!("[{ms:8}ms]"), Style::default().fg(Color::DarkGray))
    }
}

#[derive(Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ActiveInactive<T> {
    inner: T,
    active: Option<Timestamp>,
    inactive: Option<Timestamp>,
}

impl<T: Display> ActiveInactive<T> {
    fn new(item: T) -> Self {
        Self {
            inner: item,
            active: None,
            inactive: None,
        }
    }

    fn new_active(item: T, timestamp: DateTime<Utc>) -> Self {
        Self {
            inner: item,
            active: Some(timestamp.into()),
            inactive: None,
        }
    }

    fn is_active(&self) -> bool {
        self.active.is_some() && self.inactive.is_none()
    }

    fn set_active(&mut self, timestamp: DateTime<Utc>) {
        self.active = Some(timestamp.into());
        // Such that we don't have negative durations
        self.inactive = None;
    }

    fn set_inactive(&mut self, timestamp: DateTime<Utc>) {
        self.inactive = Some(timestamp.into());
    }

    // fn ms_alive(&self) -> Option<i64> {
    //     self.active.map(|active| {
    //         self.inactive
    //             .map(|inactive| active.ms_difference(inactive))
    //             .unwrap_or_else(|| active.ms_difference(Timestamp::now()))
    //     })
    // }

    fn ms_span(&self) -> Option<Span> {
        if let Some(t1) = &self.active {
            let t2 = self.inactive.unwrap_or_else(Timestamp::now);

            Some(t1.span_relative_to_ms(t2))
        } else {
            None
        }
    }

    fn to_list_item(&self) -> ListItem {
        if let Some(ms_span) = self.ms_span() {
            let mut style = Style::default();
            if self.is_active() {
                // Spans::from(vec![ms_span, Span::from(format!(" {}", self.inner))])

                // ListItem::new(format!("[{:8}ms] {}", ms_alive, self.inner)) .style(Style::default().fg(Color::Green))
                style = style.fg(Color::Green);
            } else {
                // Spans::from(vec![ms_span, Span::from(format!(" {}", self.inner))])
                // ListItem::new(format!("[{:8}ms] {}", ms_alive, self.inner))
                //     .style(Style::default().fg(Color::Red))
                style = style.fg(Color::Red);
            };

            // Spans::from(vec![ms_span, Span::from(format!(" {}", self.inner))])
            ListItem::new(Spans::from(vec![
                ms_span,
                Span::styled(format!(" {}", self.inner), style),
            ]))

        // if let Some(ms_alive) = self.ms_alive() {
        //     if self.is_active() {
        //         ListItem::new(format!("[{:8}ms] {}", ms_alive, self.inner))
        //             .style(Style::default().fg(Color::Green))
        //     } else {
        //         ListItem::new(format!("[{:8}ms] {}", ms_alive, self.inner))
        //             .style(Style::default().fg(Color::Red))
        //     }
        } else {
            ListItem::new(self.inner.to_string())
        }
    }
}

struct ActiveInactives<T>(Vec<ActiveInactive<T>>);

impl<T> Default for ActiveInactives<T> {
    fn default() -> Self {
        Self(vec![])
    }
}

// impl<T, I: IntoIterator<Item = impl Into<ActiveInactive<T>>>> From<I> for ActiveInactives<T> {
//     fn from(i: I) -> Self {
//         let mut items = vec![];
//         for item in i.into_iter() {
//             let t: ActiveInactive<T> = item.into();
//             // it.pushems(t);
//         }
//         Self(items)
//     }
// }

impl<T: PartialEq + Display> ActiveInactives<T> {
    fn list(&self) -> Vec<ListItem> {
        self.0
            .iter()
            .map(|active_inactive| {
                // if let Some(ms_alive) = active_inactive.ms_alive() {
                //     if active_inactive.is_active() {
                //         ListItem::new(format!("[{:8}ms] {}", ms_alive, active_inactive.inner))
                //             .style(Style::default().fg(Color::Green))
                //     } else {
                //         ListItem::new(format!("[{:8}ms] {}", ms_alive, active_inactive.inner))
                //             .style(Style::default().fg(Color::Red))
                //     }
                // } else {
                //     ListItem::new(active_inactive.inner.to_string())
                // }
                active_inactive.to_list_item()
            })
            .collect()
    }

    fn add_active(
        &mut self,
        items: impl IntoIterator<Item = impl Into<T>>,
        timestamp: DateTime<Utc>,
    ) {
        for item in items {
            self.0
                .push(ActiveInactive::new_active(item.into(), timestamp));
        }
    }

    fn set_inactive_if_found(
        &mut self,
        items: impl IntoIterator<Item = impl Into<T>>,
        timestamp: DateTime<Utc>,
    ) {
        for item in items {
            let item: T = item.into();
            if let Some(active_inactive) = self.0.iter_mut().find(|a| a.inner == item) {
                active_inactive.set_inactive(timestamp);
            }
        }
    }
}

struct UserState {
    first_event_timestamp: Option<Timestamp>,
    events: Vec<(serial_keel::client::Event, DateTime<Utc>)>,

    // If yes, display messages,
    // else show the list of raw events.
    show_messages: bool,

    connected: ActiveInactive<User>,
    observing: ActiveInactives<EndpointId>,
    controlling: ActiveInactives<EndpointId>,
    queued_for: ActiveInactives<EndpointId>,
}

impl UserState {
    fn new(user: User) -> Self {
        Self {
            events: Default::default(),
            connected: ActiveInactive::new(user),
            observing: Default::default(),
            controlling: Default::default(),
            queued_for: Default::default(),
            show_messages: true,
            first_event_timestamp: None,
        }
    }

    fn add_event(&mut self, event: serial_keel::client::Event, timestamp: chrono::DateTime<Utc>) {
        if self.events.is_empty() {
            self.first_event_timestamp = Some(Timestamp::new(timestamp));
        }

        use serial_keel::client::Event;

        self.events.push((event.clone(), timestamp));

        match event {
            Event::Connected => self.connected.set_active(timestamp),
            Event::Disconnected => self.connected.set_inactive(timestamp),

            Event::Observing(endpoints) => self.observing.add_active(endpoints, timestamp),

            Event::NoLongerObserving(endpoints) => {
                self.observing.set_inactive_if_found(endpoints, timestamp)
            }

            Event::InQueueFor(endpoints) => self.queued_for.add_active(endpoints, timestamp),
            Event::InControlOf(endpoints) => self.controlling.add_active(endpoints, timestamp),

            Event::NoLongerInQueueOf(endpoints) => {
                self.queued_for.set_inactive_if_found(endpoints, timestamp)
            }
            Event::NoLongerInControlOf(endpoints) => {
                self.controlling.set_inactive_if_found(endpoints, timestamp)
            }
        }
    }

    // fn user_display_name(&self, user: &User) -> ListItem {
    //     if let Some(seconds_alive) = self.connected.ms_alive() {
    //         if self.connected.is_active() {
    //             ListItem::new(format!("[{:4}s] {user}", seconds_alive))
    //                 .style(Style::default().fg(Color::Green))
    //         } else {
    //             ListItem::new(format!("[{:4}s] {user}", seconds_alive))
    //                 .style(Style::default().fg(Color::Red))
    //         }
    //     } else {
    //         ListItem::new(user.to_string())
    //     }

    // match (self.connected, self.disconnected) {
    //     (None, None) => ListItem::new(user.to_string()),
    //     (None, Some(_)) => ListItem::new(user.to_string()),
    //     (Some(connected), None) => {
    //         let time_alive = (Utc::now() - connected).num_seconds();
    //     }
    //     (Some(connected), Some(disconnected)) => {
    //         let time_alive = (disconnected - connected).num_seconds();
    //         ListItem::new(format!("[{:4}s] {user}", time_alive))
    //             .style(Style::default().fg(Color::Red))
    //     }
    // }
    // }

    fn observing_widget(&self) -> impl Widget + '_ {
        // info_list("Observing", &self.observing, &self.no_longer_observing)
        // self.observing.list()
        List::new(self.observing.list())
            .block(Block::default().title("Observing").borders(Borders::ALL))
    }

    fn controlling_widget(&self) -> impl Widget + '_ {
        // info_list(
        //     "Controlling",
        //     &self.controlling,
        //     &self.no_longer_controlling,
        // )
        List::new(self.controlling.list())
            .block(Block::default().title("Controlling").borders(Borders::ALL))
    }
    fn queued_widget(&self) -> impl Widget + '_ {
        // info_list("Queued", &self.queued_for, &self.no_longer_queued_for)
        List::new(self.queued_for.list())
            .block(Block::default().title("Queued").borders(Borders::ALL))
    }

    fn messages_widget(&self) -> List {
        let items = [
            ListItem::new("Message 1"),
            ListItem::new("Message 2"),
            ListItem::new("Message 3"),
        ];
        List::new(items)
            .block(Block::default().title("Messages [M]").borders(Borders::ALL))
            .style(Style::default().fg(Color::White))
    }

    fn events_widget(&self) -> List {
        // let first_timestamp = if let Some((_, t)) = self.events.get(0) {
        //     *t
        // } else {
        //     chrono::Utc::now()
        // };
        let make_list = |items| {
            List::new(items)
                .block(Block::default().title("Events [M]").borders(Borders::ALL))
                .style(Style::default().fg(Color::White))
        };

        let start_time = if let Some(first) = self.first_event_timestamp.as_ref() {
            first
        } else {
            return make_list(vec![]);
        };

        let items = self
            .events
            .iter()
            .map(|(event, t)| {
                ListItem::new(Spans::from(vec![
                    start_time.span_relative_to_ms(Timestamp::new(*t)),
                    Span::from(format!(" {event}")),
                ]))
            })
            .collect::<Vec<_>>();
        make_list(items)
    }

    fn messages_events_widget(&self) -> List {
        if self.show_messages {
            self.messages_widget()
        } else {
            self.events_widget()
        }
    }

    fn render<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        let top_bottom = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(10), Constraint::Min(0)].as_ref())
            .split(area);
        let top = top_bottom[0];
        let bottom = top_bottom[1];

        let left_middle_right = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                    Constraint::Min(0),
                ]
                .as_ref(),
            )
            .split(top);

        let top_left = left_middle_right[0];
        let top_middle = left_middle_right[1];
        let top_right = left_middle_right[2];

        f.render_widget(self.observing_widget(), top_left);
        f.render_widget(self.controlling_widget(), top_middle);
        f.render_widget(self.queued_widget(), top_right);

        f.render_widget(self.messages_events_widget(), bottom);
    }
}

#[derive(Default)]
struct Users {
    // Users and their data
    inner: HashMap<User, UserState>,

    //TODO?
    // users: ActiveInactives<User>,
    ui_state: ListState,
}

impl Users {
    fn selected_user(&self) -> Option<&User> {
        if let Some(selected) = self.ui_state.selected() {
            Some(self.users()[selected])
        } else {
            None
        }
    }

    fn user_state(&self, user: &User) -> &UserState {
        self.inner.get(user).unwrap()
    }

    fn user_state_mut(&mut self, user: &User) -> &mut UserState {
        self.inner.get_mut(user).unwrap()
    }

    fn add_user_event(&mut self, user_event: UserEvent) {
        self.inner
            .entry(user_event.user.clone())
            .or_insert_with(|| UserState::new(user_event.user))
            .add_event(user_event.event, user_event.timestamp);
    }

    fn char_m(&mut self) {
        let user = if let Some(selected) = self.selected_user() {
            selected.clone()
        } else {
            return;
        };

        let state = self.user_state_mut(&user);
        state.show_messages = !state.show_messages;
    }

    // fn users_widget<'s>(&'s self) -> List<'s> {
    fn users_widget(&self) -> List {
        // let mut active_inactives = ActiveInactives(vec![]);

        // for user_state in self.inner.values() {
        //     active_inactives.0.push(user_state.connected.clone());
        // }

        // List::new(active_inactives.list())
        //     .block(Block::default().title("Users").borders(Borders::ALL))

        // ActiveInactives;
        // self.inner.values()
        // let users: ActiveInactives<&User> = self
        //     .inner
        //     .values()
        //     .map(|user_state| user_state.connected)
        //     .into();

        let list_items = self
            .inner
            .values()
            .map(|user_state| user_state.connected.to_list_item())
            .collect::<Vec<_>>();

        List::new(list_items)
            .block(Block::default().borders(Borders::all()).title("Users [↑↓]"))
            .highlight_style(Style::default().add_modifier(Modifier::BOLD))

        // todo!()
    }

    fn users(&self) -> Vec<&User> {
        self.inner.keys().collect()
    }

    fn next(&mut self) {
        let users = self.users();

        let i = match self.ui_state.selected() {
            Some(i) => {
                if i == self.users().len() - 1 {
                    Some(0)
                } else {
                    Some(i + 1)
                }
            }
            None => {
                if users.is_empty() {
                    None
                } else {
                    Some(0)
                }
            }
        };
        self.ui_state.select(i);
    }

    fn previous(&mut self) {
        let users = self.users();

        let i = match self.ui_state.selected() {
            Some(i) => {
                if i == 0 {
                    Some(users.len() - 1)
                } else {
                    Some(i - 1)
                }
            }
            None => {
                if users.is_empty() {
                    None
                } else {
                    Some(users.len() - 1)
                }
            }
        };
        self.ui_state.select(i);
    }

    fn render<B: Backend>(&mut self, f: &mut Frame<B>, area: Rect) {
        let left_right = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(15), Constraint::Min(0)].as_ref())
            .split(area);
        let left = left_right[0];
        let right = left_right[1];

        // let users = self
        //     .inner
        //     .iter()
        //     .map(|(user, state)| state.user_display_name(user))
        //     .collect::<Vec<_>>();

        // To avoid immut+mut borrow.
        // Why is state borrowed mutably anyway?
        let mut state = self.ui_state.clone();

        f.render_stateful_widget(
            // List::new(users)
            //     .block(Block::default().borders(Borders::all()).title("Users [↑↓]"))
            //     .highlight_style(Style::default().add_modifier(Modifier::BOLD)),
            self.users_widget(),
            left,
            &mut state,
        );

        if let Some(selected) = self.selected_user() {
            let user_state = self.user_state(selected);
            user_state.render(f, right);
        }
    }
}

/// This struct holds the current state of the app. In particular, it has the `items` field which is a wrapper
/// around `ListState`. Keeping track of the items state let us render the associated widget with its state
/// and have access to features such as natural scrolling.
///
/// Check the event handling at the bottom to see how to change the state on incoming events.
/// Check the drawing logic for items on how to specify the highlighting style for selected items.
struct App {
    tab: Tab,
    raw_events: Vec<UserEvent>,
    users: Users,
}

impl App {
    fn new() -> App {
        App {
            raw_events: vec![],
            tab: Tab::Serial,
            users: Users::default(),
        }
    }

    fn up(&mut self) {
        match self.tab {
            Tab::Serial | Tab::Server => {}
            Tab::Users => {
                self.users.next();
            }
        }
    }

    fn down(&mut self) {
        match self.tab {
            Tab::Serial | Tab::Server => {}
            Tab::Users => {
                self.users.previous();
            }
        }
    }

    fn char_m(&mut self) {
        match self.tab {
            Tab::Serial | Tab::Server => {}
            Tab::Users => {
                self.users.char_m();
            }
        }
    }

    fn add_user_event(&mut self, event: UserEvent) {
        self.raw_events.push(event.clone());
        self.users.add_user_event(event);
    }

    fn next_tab(&mut self) {
        self.tab = enum_iterator::next_cycle(&self.tab).unwrap();
    }

    fn previous_tab(&mut self) {
        self.tab = enum_iterator::previous_cycle(&self.tab).unwrap();
    }

    fn tabs(&self) -> impl Iterator<Item = Tab> {
        enum_iterator::all::<Tab>()
    }

    fn tab_index(&self) -> usize {
        self.tab.index()
    }

    fn ui_serial<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        ui_example(f, area, Color::Blue)
    }
    fn ui_users<B: Backend>(&mut self, f: &mut Frame<B>, area: Rect) {
        // TODO: Left side is a list of users, right side is the user state for the selected user.
        // ui_example(f, area, Color::Red)
        self.users.render(f, area);
    }
    fn ui_server<B: Backend>(&self, f: &mut Frame<B>, area: Rect) {
        ui_example(f, area, Color::Green)
    }

    fn tab_widget<B: Backend>(&mut self, f: &mut Frame<B>, area: Rect) {
        match self.tab {
            Tab::Serial => self.ui_serial(f, area),
            Tab::Users => self.ui_users(f, area),
            Tab::Server => self.ui_server(f, area),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // connect to server
    let mut sk_client = serial_keel::client::ClientHandle::new("localhost", 3123).await?;
    let user_events = sk_client.observe_user_events().await?;

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let tick_rate = Duration::from_millis(50);
    let app = App::new();
    let res = run_app(&mut terminal, app, user_events, tick_rate);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
    mut user_events: UserEventReader,
    tick_rate: Duration,
) -> io::Result<()> {
    let mut last_tick = Instant::now();
    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());

        if event::poll(timeout)? {
            if let event::Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Left | KeyCode::Char('h') => app.previous_tab(),
                    KeyCode::Right | KeyCode::Char('l') => app.next_tab(),
                    KeyCode::Down => app.up(),
                    KeyCode::Up => app.down(),
                    KeyCode::Char('m') => app.char_m(),
                    _ => {}
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        while let Some(user_event) = user_events.try_next_user_event() {
            app.add_user_event(user_event);
        }
    }
}

// fn widget_serial() {}

fn ui<B: Backend>(f: &mut Frame<B>, app: &mut App) {
    // Create two chunks with equal horizontal screen space
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
        .split(f.size());

    let titles = app
        .tabs()
        .map(|t| {
            Spans::from(vec![Span::styled(
                format!("{:?}", t),
                Style::default().fg(Color::LightGreen),
            )])
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Serial Keel TUI"),
        )
        .select(app.tab_index())
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED));

    f.render_widget(tabs, chunks[0]);

    app.tab_widget(f, chunks[1]);
}
