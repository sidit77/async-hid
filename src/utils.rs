#![allow(dead_code)]

use std::iter::Fuse;

pub trait TryIterExt<T, E> {
    fn try_collect_vec(self) -> Result<Vec<T>, E>;

    fn try_flatten(self) -> TryFlattenIter<Self, T>
    where
        T: IntoIterator,
        Self: Sized;
}

impl<T, E, I: Iterator<Item = Result<T, E>>> TryIterExt<T, E> for I {
    fn try_collect_vec(self) -> Result<Vec<T>, E> {
        let mut result = Vec::with_capacity(self.size_hint().0);
        for elem in self {
            result.push(elem?);
        }
        Ok(result)
    }
    fn try_flatten(self) -> TryFlattenIter<Self, T>
    where
        T: IntoIterator,
    {
        TryFlattenIter {
            inner: self.fuse(),
            current: None,
        }
    }
}

pub struct TryFlattenIter<I, T: IntoIterator> {
    inner: Fuse<I>,
    current: Option<T::IntoIter>,
}

impl<I, T, E> Iterator for TryFlattenIter<I, T>
where
    I: Iterator<Item = Result<T, E>>,
    T: IntoIterator,
{
    type Item = Result<T::Item, E>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(current) = self.current.as_mut().and_then(Iterator::next) {
                return Some(Ok(current));
            }
            self.current = None;
            match self.inner.next()? {
                Ok(iter) => self.current = Some(iter.into_iter()),
                Err(err) => return Some(Err(err)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::utils::TryFlattenIter;

    #[test]
    fn test_try_flatten_iter() {
        let iter = TryFlattenIter::<_, Vec<i32>> {
            inner: [Err(0), Ok(vec![1, 2, 3]), Err(4), Ok(vec![5, 6, 7]), Err(8), Err(9)]
                .into_iter()
                .fuse(),
            current: None,
        };
        assert_eq!(
            iter.collect::<Vec<_>>(),
            vec![Err(0), Ok(1), Ok(2), Ok(3), Err(4), Ok(5), Ok(6), Ok(7), Err(8), Err(9)]
        );
    }
}
