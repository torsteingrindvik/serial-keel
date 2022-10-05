use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::{channel::mpsc, sink, stream, Sink, SinkExt, Stream, StreamExt, TryFutureExt};
use nordic_types::serial::SerialMessage;
use tokio::sync::{broadcast, Mutex};

use super::{Endpoint, EndpointHandle};

struct Mock {
    data: stream::Iter<std::vec::IntoIter<SerialMessage>>,
    dumpster: sink::Drain<SerialMessage>,
}

impl Mock {
    fn run(data: Vec<SerialMessage>) -> EndpointHandle {
        let (arriving_messages_sender, arriving_messages_receiver) = broadcast::channel(1024);
        let (messages_to_send_sender, messages_to_send_receiver) = mpsc::unbounded();

        struct MyReciever(broadcast::Receiver<SerialMessage>);

        impl Stream for MyReciever {
            type Item = SerialMessage;

            fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
                let fut = self.0.recv();
            }
        }

        tokio::spawn(async move {
            let both = futures::stream::select(
                arriving_messages_receiver.recv(),
                messages_to_send_receiver,
            );
        });

        // Self {
        //     data: stream::iter(data),
        //     dumpster: sink::drain(),
        // }

        EndpointHandle {
            arriving_messages: arriving_messages_sender,
            messages_to_send: Arc::new(Mutex::new(messages_to_send_sender)),
        }
    }
}

impl Endpoint for Mock {
    fn handle(&self) -> super::EndpointHandle {
        todo!()
    }
}

impl Sink<SerialMessage> for Mock {
    type Error = Infallible;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.dumpster.poll_ready_unpin(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: SerialMessage) -> Result<(), Self::Error> {
        self.dumpster.start_send_unpin(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.dumpster.poll_flush_unpin(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.dumpster.poll_close_unpin(cx)
    }
}

impl Stream for Mock {
    type Item = SerialMessage;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.data.poll_next_unpin(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::{SinkExt, StreamExt};

    #[tokio::test]
    async fn mock_produces_output() {
        let mut mock = Mock::run(vec!["a".into(), "b".into()]);

        let a = mock.next().await;
        assert_eq!(a, Some(SerialMessage::from("a")));

        let b = mock.next().await;
        assert_eq!(b, Some(SerialMessage::from("b")));

        let c = mock.next().await;
        assert_eq!(c, None);
    }

    #[tokio::test]
    async fn mock_receives_input() {
        let mut mock = Mock::run(vec![]);

        for message in ["a", "foo", ""].map(String::from) {
            mock.send(message).await.expect("Should receive");
        }
    }
}
