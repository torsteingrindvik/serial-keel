pub mod state {
    use std::sync::RwLockReadGuard;
    use std::sync::{Arc, RwLock};

    use std::sync::RwLockWriteGuard;

    #[derive(Debug, Default)]
    pub struct SharedState<T: Default> {
        inner: Arc<RwLock<T>>,
    }

    impl<T: Default> Clone for SharedState<T> {
        fn clone(&self) -> Self {
            Self {
                inner: self.inner.clone(),
            }
        }
    }

    impl<T: Default> SharedState<T> {
        pub fn state_mut(&mut self) -> RwLockWriteGuard<T> {
            self.inner
                .try_write()
                .expect("Should not try writing when not having exclusive access")
        }

        pub fn state(&self) -> RwLockReadGuard<T> {
            self.inner
                .try_read()
                .expect("Should not try reading when not having shared access")
        }
    }
}

pub mod fonts {
    use iced::Font;
    use iced_aw::graphics;

    pub const ICONS: Font = graphics::icons::ICON_FONT;

    pub const MONO: Font = Font::External {
        name: "FiraMono",
        bytes: include_bytes!("../assets/fonts/mono/FiraMono-Medium.ttf"),
    };
}

pub mod containers {
    use iced::{widget::container, Element, Length};

    pub fn fill<'a, T: 'a>(content: impl Into<Element<'a, T>>) -> Element<'a, T> {
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub fn fill_centered_xy<'a, T: 'a>(content: impl Into<Element<'a, T>>) -> Element<'a, T> {
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .center_y()
            .into()
    }
}

pub mod elements {
    use iced::{widget::text, Element};

    use super::containers;

    pub fn empty<'a, T: 'a>(message: &'a str) -> Element<'a, T> {
        containers::fill_centered_xy(text(message).size(32))
    }
}

pub mod style {
    use iced::{widget::container, Theme};

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

pub mod time {
    use serial_keel::client::{DateTime, Utc};

    pub fn human_readable_absolute(date_time: &DateTime<Utc>) -> String {
        date_time.time().format("%H:%M:%S%.3f").to_string()
    }

    pub fn human_readable_relative(
        relative_start: &DateTime<Utc>,
        date_time: &DateTime<Utc>,
    ) -> String {
        let secs = date_time
            .signed_duration_since(*relative_start)
            .to_std()
            .unwrap()
            .as_secs_f32();

        format!("{secs:10.3}")
    }
}
