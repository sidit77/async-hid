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


pub trait TryIterExt<T, E> {
    fn try_collect_vec(self) -> Result<Vec<T>, E>;
}

impl<T, E, I: Iterator<Item = Result<T, E>>, > TryIterExt<T, E> for I {
    fn try_collect_vec(self) -> Result<Vec<T>, E> {
        let mut result = Vec::with_capacity(self.size_hint().0);
        for elem in self {
            result.push(elem?);
        }
        Ok(result)
    }
}