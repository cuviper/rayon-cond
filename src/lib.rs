extern crate either;
extern crate rayon;

use either::{Either, Left, Right};
use rayon::prelude::*;

use rayon::iter as ri;
use std::iter as si;

/// An iterator that could be parallel or serial, with a common API either way.
pub struct CondIterator<P, S>
where
    P: ParallelIterator,
    S: Iterator<Item = P::Item>,
{
    inner: Either<P, S>,
}

impl<P, S> CondIterator<P, S>
where
    P: ParallelIterator,
    S: Iterator<Item = P::Item>,
{
    pub fn new<I>(iterable: I, parallel: bool) -> Self
    where
        I: IntoParallelIterator<Iter = P, Item = P::Item>
            + IntoIterator<IntoIter = S, Item = S::Item>,
    {
        if parallel {
            Self::from_par_iter(iterable)
        } else {
            Self::from_iter(iterable)
        }
    }

    pub fn from_par_iter<I>(iterable: I) -> Self
    where
        I: IntoParallelIterator<Iter = P, Item = P::Item>,
    {
        CondIterator {
            inner: Left(iterable.into_par_iter()),
        }
    }

    pub fn from_iter<I>(iterable: I) -> Self
    where
        I: IntoIterator<IntoIter = S, Item = S::Item>,
    {
        CondIterator {
            inner: Right(iterable.into_iter()),
        }
    }

    pub fn is_parallel(&self) -> bool {
        self.inner.is_left()
    }

    pub fn is_serial(&self) -> bool {
        self.inner.is_right()
    }
}

macro_rules! either {
    ($self:ident, $pattern:pat => $result:expr) => {
        match $self.inner {
            Either::Left($pattern) => $result,
            Either::Right($pattern) => $result,
        }
    };
}

macro_rules! wrap_either {
    ($self:ident, $pattern:pat => $result:expr) => {
        CondIterator {
            inner: match $self.inner {
                Left($pattern) => Left($result),
                Right($pattern) => Right($result),
            },
        }
    };
}

impl<P, S> CondIterator<P, S>
where
    P: ParallelIterator,
    S: Iterator<Item = P::Item>,
{
    pub fn for_each<OP>(self, op: OP)
    where
        OP: Fn(P::Item) + Sync + Send,
    {
        either!(self, iter => iter.for_each(op))
    }

    pub fn for_each_with<OP, T>(self, mut init: T, op: OP)
    where
        OP: Fn(&mut T, P::Item) + Sync + Send,
        T: Send + Clone,
    {
        match self.inner {
            Left(iter) => iter.for_each_with(init, op),
            Right(iter) => iter.for_each(move |item| op(&mut init, item)),
        }
    }

    pub fn count(self) -> usize {
        either!(self, iter => iter.count())
    }

    pub fn map<F, R>(self, map_op: F) -> CondIterator<ri::Map<P, F>, si::Map<S, F>>
    where
        F: Fn(P::Item) -> R + Sync + Send,
        R: Send,
    {
        wrap_either!(self, iter => iter.map(map_op))
    }

    pub fn sum<Sum>(self) -> Sum
    where
        Sum: Send + si::Sum<P::Item> + si::Sum<Sum>,
    {
        either!(self, iter => iter.sum())
    }

    pub fn product<Product>(self) -> Product
    where
        Product: Send + si::Product<P::Item> + si::Product<Product>,
    {
        either!(self, iter => iter.product())
    }

    pub fn collect<C>(self) -> C
    where
        C: FromParallelIterator<P::Item> + si::FromIterator<S::Item>,
    {
        either!(self, iter => iter.collect())
    }
}

impl<P, S> CondIterator<P, S>
where
    P: IndexedParallelIterator,
    S: Iterator<Item = P::Item>,
{
    pub fn zip<Z>(self, other: Z) -> CondIterator<ri::Zip<P, Z::Iter>, si::Zip<S, Z::IntoIter>>
    where
        Z: IntoParallelIterator + IntoIterator<Item = <Z as IntoParallelIterator>::Item>,
        Z::Iter: IndexedParallelIterator,
    {
        wrap_either!(self, iter => iter.zip(other))
    }

    pub fn enumerate(self) -> CondIterator<ri::Enumerate<P>, si::Enumerate<S>> {
        wrap_either!(self, iter => iter.enumerate())
    }
}
