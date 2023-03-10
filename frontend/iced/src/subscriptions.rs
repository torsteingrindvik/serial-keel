// use iced::{futures::stream::BoxStream, subscription};
// use serial_keel::{client::EventReader, events::TimestampedEvent};

// #[derive(Debug)]
// struct SerialKeelEvents(EventReader);

// Check the websocket example (and the iced PR that added it) for mpsc tips.

// impl<H, E> subscription::Recipe<H, E> for SerialKeelEvents
// where
//     H: std::hash::Hasher,
// {
//     type Output = TimestampedEvent;

//     fn hash(&self, state: &mut H) {
//         use std::hash::Hash;
//         std::any::TypeId::of::<Self>().hash(state);
//     }

//     fn stream(mut self: Box<Self>, input: BoxStream<E>) -> BoxStream<Self::Output> {
//         self.0.box_stream()
//     }
// }
