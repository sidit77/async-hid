use std::pin::Pin;
use std::task::{Context, Poll};
use futures_core::Stream;

pub fn iter<I: IntoIterator>(iter: I) -> Iter<I::IntoIter> {
    Iter {
        iter: iter.into_iter(),
    }
}

#[derive(Clone, Debug)]
#[must_use = "streams do nothing unless polled"]
pub struct Iter<I> {
    iter: I,
}

impl<I> Unpin for Iter<I> {}

impl<I: Iterator> Stream for Iter<I> {
    type Item = I::Item;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.iter.next())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}