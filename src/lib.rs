//! Experimental iterator wrapper that is conditionally parallel or serial, using
//! Rayon's `ParallelIterator` or the standard `Iterator` respectively.
//!
//! ## Usage
//!
//! First add this crate to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! rayon-cond = "0.1"
//! ```
//!
//! Then in your code, it may be used something like this:
//!
//! ```rust
//! extern crate rayon_cond;
//!
//! use rayon_cond::CondIterator;
//!
//! fn main() {
//!     let args: Vec<_> = std::env::args().collect();
//!
//!     // Run in parallel if there are an even number of args
//!     let par = args.len() % 2 == 0;
//!
//!     CondIterator::new(args, par).enumerate().for_each(|(i, arg)| {
//!         println!("arg {}: {:?}", i, arg);
//!     });
//! }
//! ```

extern crate either;
extern crate itertools;
extern crate rayon;

use either::Either;
use itertools::Itertools;
use rayon::prelude::*;
use std::cmp::Ordering;

use itertools::structs as it;
use rayon::iter as ri;
use std::iter as si;

use EitherIterator::*;

/// An iterator that could be parallel or serial, with a common API either way.
pub struct CondIterator<P, S>
where
    P: ParallelIterator,
    S: Iterator<Item = P::Item>,
{
    inner: EitherIterator<P, S>,
}

enum EitherIterator<P, S>
where
    P: ParallelIterator,
    S: Iterator<Item = P::Item>,
{
    Parallel(P),
    Serial(S),
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
            inner: Parallel(iterable.into_par_iter()),
        }
    }

    pub fn from_iter<I>(iterable: I) -> Self
    where
        I: IntoIterator<IntoIter = S, Item = S::Item>,
    {
        CondIterator {
            inner: Serial(iterable.into_iter()),
        }
    }

    pub fn is_parallel(&self) -> bool {
        match self.inner {
            Parallel(_) => true,
            _ => false,
        }
    }

    pub fn is_serial(&self) -> bool {
        match self.inner {
            Serial(_) => true,
            _ => false,
        }
    }
}

impl<P, S> CondIterator<P, S>
where
    P: ParallelIterator,
    S: Iterator<Item = P::Item> + Send,
{
    pub fn into_parallel(self) -> Either<P, ri::IterBridge<S>> {
        match self.inner {
            Parallel(iter) => Either::Left(iter),
            Serial(iter) => Either::Right(iter.par_bridge()),
        }
    }
}

macro_rules! either {
    ($self:ident, $pattern:pat => $result:expr) => {
        match $self.inner {
            Parallel($pattern) => $result,
            Serial($pattern) => $result,
        }
    };
}

macro_rules! wrap_either {
    ($self:ident, $pattern:pat => $result:expr) => {
        CondIterator {
            inner: match $self.inner {
                Parallel($pattern) => Parallel($result),
                Serial($pattern) => Serial($result),
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
            Parallel(iter) => iter.for_each_with(init, op),
            Serial(iter) => iter.for_each(move |item| op(&mut init, item)),
        }
    }

    pub fn for_each_init<OP, INIT, T>(self, init: INIT, op: OP)
    where
        OP: Fn(&mut T, P::Item) + Sync + Send,
        INIT: Fn() -> T + Sync + Send,
    {
        match self.inner {
            Parallel(iter) => iter.for_each_init(init, op),
            Serial(iter) => {
                let mut init = init();
                iter.for_each(move |item| op(&mut init, item))
            }
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

    // If we want to avoid `impl FnMut`, we'll need to implement a custom
    // serialized `MapWith` type to return.
    pub fn map_with<F, T, R>(
        self,
        mut init: T,
        map_op: F,
    ) -> CondIterator<ri::MapWith<P, T, F>, si::Map<S, impl FnMut(P::Item) -> R>>
    where
        F: Fn(&mut T, P::Item) -> R + Sync + Send,
        T: Send + Clone,
        R: Send,
    {
        CondIterator {
            inner: match self.inner {
                Parallel(iter) => Parallel(iter.map_with(init, map_op)),
                Serial(iter) => Serial(iter.map(move |item| map_op(&mut init, item))),
            },
        }
    }

    // If we want to avoid `impl FnMut`, we'll need to implement a custom
    // serialized `MapInit` type to return.
    pub fn map_init<F, INIT, T, R>(
        self,
        init: INIT,
        map_op: F,
    ) -> CondIterator<ri::MapInit<P, INIT, F>, si::Map<S, impl FnMut(P::Item) -> R>>
    where
        F: Fn(&mut T, P::Item) -> R + Sync + Send,
        INIT: Fn() -> T + Sync + Send,
        R: Send,
    {
        CondIterator {
            inner: match self.inner {
                Parallel(iter) => Parallel(iter.map_init(init, map_op)),
                Serial(iter) => {
                    let mut init = init();
                    Serial(iter.map(move |item| map_op(&mut init, item)))
                }
            },
        }
    }

    pub fn cloned<'a, T>(self) -> CondIterator<ri::Cloned<P>, si::Cloned<S>>
    where
        T: 'a + Clone + Sync + Send,
        P: ParallelIterator<Item = &'a T>,
        S: Iterator<Item = &'a T>,
    {
        wrap_either!(self, iter => iter.cloned())
    }

    pub fn inspect<OP>(self, inspect_op: OP) -> CondIterator<ri::Inspect<P, OP>, si::Inspect<S, OP>>
    where
        OP: Fn(&P::Item) + Sync + Send,
    {
        wrap_either!(self, iter => iter.inspect(inspect_op))
    }

    pub fn update<OP>(self, update_op: OP) -> CondIterator<ri::Update<P, OP>, it::Update<S, OP>>
    where
        OP: Fn(&mut P::Item) + Sync + Send,
    {
        wrap_either!(self, iter => iter.update(update_op))
    }

    pub fn filter<Pred>(
        self,
        filter_op: Pred,
    ) -> CondIterator<ri::Filter<P, Pred>, si::Filter<S, Pred>>
    where
        Pred: Fn(&P::Item) -> bool + Sync + Send,
    {
        wrap_either!(self, iter => iter.filter(filter_op))
    }

    pub fn filter_map<Pred, R>(
        self,
        filter_op: Pred,
    ) -> CondIterator<ri::FilterMap<P, Pred>, si::FilterMap<S, Pred>>
    where
        Pred: Fn(P::Item) -> Option<R> + Sync + Send,
        R: Send,
    {
        wrap_either!(self, iter => iter.filter_map(filter_op))
    }

    pub fn flat_map<F, I>(self, map_op: F) -> CondIterator<ri::FlatMap<P, F>, si::FlatMap<S, I, F>>
    where
        F: Fn(P::Item) -> I + Sync + Send,
        I: IntoParallelIterator + IntoIterator<Item = <I as IntoParallelIterator>::Item>,
    {
        wrap_either!(self, iter => iter.flat_map(map_op))
    }

    pub fn flatten(self) -> CondIterator<ri::Flatten<P>, si::Flatten<S>>
    where
        P::Item: IntoParallelIterator,
        S::Item: IntoIterator<Item = <P::Item as IntoParallelIterator>::Item>,
    {
        wrap_either!(self, iter => iter.flatten())
    }

    pub fn reduce<OP, ID>(self, identity: ID, op: OP) -> P::Item
    where
        OP: Fn(P::Item, P::Item) -> P::Item + Sync + Send,
        ID: Fn() -> P::Item + Sync + Send,
    {
        match self.inner {
            Parallel(iter) => iter.reduce(identity, op),
            Serial(iter) => iter.fold(identity(), op),
        }
    }

    pub fn reduce_with<OP>(self, op: OP) -> Option<P::Item>
    where
        OP: Fn(P::Item, P::Item) -> P::Item + Sync + Send,
    {
        match self.inner {
            Parallel(iter) => iter.reduce_with(op),
            Serial(iter) => iter.fold(None, |acc, item| match acc {
                Some(acc) => Some(op(acc, item)),
                None => Some(item),
            }),
        }
    }

    // NB: Rayon's `fold` produces another iterator, so we have to fake that serially.
    pub fn fold<T, ID, F>(
        self,
        identity: ID,
        fold_op: F,
    ) -> CondIterator<ri::Fold<P, ID, F>, si::Once<T>>
    where
        F: Fn(T, P::Item) -> T + Sync + Send,
        ID: Fn() -> T + Sync + Send,
        T: Send,
    {
        CondIterator {
            inner: match self.inner {
                Parallel(iter) => Parallel(iter.fold(identity, fold_op)),
                Serial(iter) => Serial(si::once(iter.fold(identity(), fold_op))),
            },
        }
    }

    pub fn fold_with<F, T>(
        self,
        init: T,
        fold_op: F,
    ) -> CondIterator<ri::FoldWith<P, T, F>, si::Once<T>>
    where
        F: Fn(T, P::Item) -> T + Sync + Send,
        T: Send + Clone,
    {
        CondIterator {
            inner: match self.inner {
                Parallel(iter) => Parallel(iter.fold_with(init, fold_op)),
                Serial(iter) => Serial(si::once(iter.fold(init, fold_op))),
            },
        }
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

    pub fn min(self) -> Option<P::Item>
    where
        P::Item: Ord,
    {
        either!(self, iter => iter.min())
    }

    pub fn min_by<F>(self, f: F) -> Option<P::Item>
    where
        F: Sync + Send + Fn(&P::Item, &P::Item) -> Ordering,
    {
        either!(self, iter => iter.min_by(f))
    }

    pub fn min_by_key<K, F>(self, f: F) -> Option<P::Item>
    where
        K: Ord + Send,
        F: Sync + Send + Fn(&P::Item) -> K,
    {
        either!(self, iter => iter.min_by_key(f))
    }

    pub fn max(self) -> Option<P::Item>
    where
        P::Item: Ord,
    {
        either!(self, iter => iter.max())
    }

    pub fn max_by<F>(self, f: F) -> Option<P::Item>
    where
        F: Sync + Send + Fn(&P::Item, &P::Item) -> Ordering,
    {
        either!(self, iter => iter.max_by(f))
    }

    pub fn max_by_key<K, F>(self, f: F) -> Option<P::Item>
    where
        K: Ord + Send,
        F: Sync + Send + Fn(&P::Item) -> K,
    {
        either!(self, iter => iter.max_by_key(f))
    }

    pub fn chain<C>(
        self,
        chain: C,
    ) -> CondIterator<ri::Chain<P, C::Iter>, si::Chain<S, C::IntoIter>>
    where
        C: IntoParallelIterator<Item = P::Item> + IntoIterator<Item = P::Item>,
    {
        wrap_either!(self, iter => iter.chain(chain))
    }

    pub fn find_any<Pred>(self, predicate: Pred) -> Option<P::Item>
    where
        Pred: Fn(&P::Item) -> bool + Sync + Send,
    {
        match self.inner {
            Parallel(iter) => iter.find_any(predicate),
            Serial(mut iter) => iter.find(predicate),
        }
    }

    pub fn find_first<Pred>(self, predicate: Pred) -> Option<P::Item>
    where
        Pred: Fn(&P::Item) -> bool + Sync + Send,
    {
        match self.inner {
            Parallel(iter) => iter.find_first(predicate),
            Serial(mut iter) => iter.find(predicate),
        }
    }

    pub fn any<Pred>(self, predicate: Pred) -> bool
    where
        Pred: Fn(P::Item) -> bool + Sync + Send,
    {
        match self.inner {
            Parallel(iter) => iter.any(predicate),
            Serial(mut iter) => iter.any(predicate),
        }
    }

    pub fn all<Pred>(self, predicate: Pred) -> bool
    where
        Pred: Fn(P::Item) -> bool + Sync + Send,
    {
        match self.inner {
            Parallel(iter) => iter.all(predicate),
            Serial(mut iter) => iter.all(predicate),
        }
    }

    pub fn while_some<T>(self) -> CondIterator<ri::WhileSome<P>, it::WhileSome<S>>
    where
        P: ParallelIterator<Item = Option<T>>,
        S: Iterator<Item = Option<T>>,
        T: Send,
    {
        wrap_either!(self, iter => iter.while_some())
    }

    pub fn collect<C>(self) -> C
    where
        C: FromParallelIterator<P::Item> + si::FromIterator<S::Item>,
    {
        either!(self, iter => iter.collect())
    }

    pub fn unzip<A, B, FromA, FromB>(self) -> (FromA, FromB)
    where
        P: ParallelIterator<Item = (A, B)>,
        S: Iterator<Item = (A, B)>,
        FromA: Default + Send + ParallelExtend<A> + Extend<A>,
        FromB: Default + Send + ParallelExtend<B> + Extend<B>,
        A: Send,
        B: Send,
    {
        either!(self, iter => iter.unzip())
    }

    // NB: `Iterator::partition` only allows a single output type
    pub fn partition<B, Pred>(self, predicate: Pred) -> (B, B)
    where
        B: Default + Send + ParallelExtend<P::Item> + Extend<S::Item>,
        Pred: Fn(&P::Item) -> bool + Sync + Send,
    {
        either!(self, iter => iter.partition(predicate))
    }

    pub fn partition_map<A, B, Pred, L, R>(self, predicate: Pred) -> (A, B)
    where
        A: Default + Send + ParallelExtend<L> + Extend<L>,
        B: Default + Send + ParallelExtend<R> + Extend<R>,
        Pred: Fn(P::Item) -> Either<L, R> + Sync + Send,
        L: Send,
        R: Send,
    {
        either!(self, iter => iter.partition_map(predicate))
    }

    pub fn intersperse(
        self,
        element: P::Item,
    ) -> CondIterator<ri::Intersperse<P>, it::Intersperse<S>>
    where
        P::Item: Clone,
    {
        wrap_either!(self, iter => iter.intersperse(element))
    }

    pub fn opt_len(&self) -> Option<usize> {
        match self.inner {
            Parallel(ref iter) => iter.opt_len(),
            Serial(ref iter) => match iter.size_hint() {
                (lo, Some(hi)) if lo == hi => Some(lo),
                _ => None,
            },
        }
    }
}

impl<P, S> CondIterator<P, S>
where
    P: ParallelIterator,
    S: DoubleEndedIterator<Item = P::Item>,
{
    pub fn find_last<Pred>(self, predicate: Pred) -> Option<P::Item>
    where
        Pred: Fn(&P::Item) -> bool + Sync + Send,
    {
        match self.inner {
            Parallel(iter) => iter.find_last(predicate),
            Serial(mut iter) => iter.rfind(predicate),
        }
    }
}

impl<P, S> CondIterator<P, S>
where
    P: IndexedParallelIterator,
    S: Iterator<Item = P::Item>,
{
    pub fn collect_into_vec(self, target: &mut Vec<P::Item>) {
        match self.inner {
            Parallel(iter) => iter.collect_into_vec(target),
            Serial(iter) => {
                target.clear();
                let (lower, _) = iter.size_hint();
                target.reserve(lower);
                target.extend(iter);
            }
        }
    }

    pub fn unzip_into_vecs<A, B>(self, left: &mut Vec<A>, right: &mut Vec<B>)
    where
        P: IndexedParallelIterator<Item = (A, B)>,
        S: Iterator<Item = (A, B)>,
        A: Send,
        B: Send,
    {
        match self.inner {
            Parallel(iter) => iter.unzip_into_vecs(left, right),
            Serial(iter) => {
                left.clear();
                right.clear();
                let (lower, _) = iter.size_hint();
                left.reserve(lower);
                left.reserve(lower);
                iter.for_each(|(a, b)| {
                    left.push(a);
                    right.push(b);
                })
            }
        }
    }

    pub fn zip<Z>(self, other: Z) -> CondIterator<ri::Zip<P, Z::Iter>, si::Zip<S, Z::IntoIter>>
    where
        Z: IntoParallelIterator + IntoIterator<Item = <Z as IntoParallelIterator>::Item>,
        Z::Iter: IndexedParallelIterator,
    {
        wrap_either!(self, iter => iter.zip(other))
    }

    pub fn zip_eq<Z>(
        self,
        other: Z,
    ) -> CondIterator<ri::ZipEq<P, Z::Iter>, it::ZipEq<S, Z::IntoIter>>
    where
        Z: IntoParallelIterator + IntoIterator<Item = <Z as IntoParallelIterator>::Item>,
        Z::Iter: IndexedParallelIterator,
    {
        wrap_either!(self, iter => iter.zip_eq(other))
    }

    pub fn interleave<I>(
        self,
        other: I,
    ) -> CondIterator<ri::Interleave<P, I::Iter>, it::Interleave<S, I::IntoIter>>
    where
        I: IntoParallelIterator<Item = P::Item> + IntoIterator<Item = S::Item>,
        I::Iter: IndexedParallelIterator<Item = P::Item>,
    {
        wrap_either!(self, iter => iter.interleave(other))
    }

    pub fn interleave_shortest<I>(
        self,
        other: I,
    ) -> CondIterator<ri::InterleaveShortest<P, I::Iter>, it::InterleaveShortest<S, I::IntoIter>>
    where
        I: IntoParallelIterator<Item = P::Item> + IntoIterator<Item = S::Item>,
        I::Iter: IndexedParallelIterator<Item = P::Item>,
    {
        wrap_either!(self, iter => iter.interleave_shortest(other))
    }

    pub fn cmp<I>(self, other: I) -> Ordering
    where
        I: IntoParallelIterator<Item = P::Item> + IntoIterator<Item = S::Item>,
        I::Iter: IndexedParallelIterator,
        P::Item: Ord,
    {
        either!(self, iter => iter.cmp(other))
    }

    pub fn partial_cmp<I>(self, other: I) -> Option<Ordering>
    where
        I: IntoParallelIterator + IntoIterator<Item = <I as IntoParallelIterator>::Item>,
        I::Iter: IndexedParallelIterator,
        P::Item: PartialOrd<<I as IntoParallelIterator>::Item>,
    {
        either!(self, iter => iter.partial_cmp(other))
    }

    pub fn eq<I>(self, other: I) -> bool
    where
        I: IntoParallelIterator + IntoIterator<Item = <I as IntoParallelIterator>::Item>,
        I::Iter: IndexedParallelIterator,
        P::Item: PartialEq<<I as IntoParallelIterator>::Item>,
    {
        either!(self, iter => iter.eq(other))
    }

    pub fn ne<I>(self, other: I) -> bool
    where
        I: IntoParallelIterator + IntoIterator<Item = <I as IntoParallelIterator>::Item>,
        I::Iter: IndexedParallelIterator,
        P::Item: PartialEq<<I as IntoParallelIterator>::Item>,
    {
        either!(self, iter => iter.ne(other))
    }

    pub fn lt<I>(self, other: I) -> bool
    where
        I: IntoParallelIterator + IntoIterator<Item = <I as IntoParallelIterator>::Item>,
        I::Iter: IndexedParallelIterator,
        P::Item: PartialOrd<<I as IntoParallelIterator>::Item>,
    {
        either!(self, iter => iter.lt(other))
    }

    pub fn le<I>(self, other: I) -> bool
    where
        I: IntoParallelIterator + IntoIterator<Item = <I as IntoParallelIterator>::Item>,
        I::Iter: IndexedParallelIterator,
        P::Item: PartialOrd<<I as IntoParallelIterator>::Item>,
    {
        either!(self, iter => iter.le(other))
    }

    pub fn gt<I>(self, other: I) -> bool
    where
        I: IntoParallelIterator + IntoIterator<Item = <I as IntoParallelIterator>::Item>,
        I::Iter: IndexedParallelIterator,
        P::Item: PartialOrd<<I as IntoParallelIterator>::Item>,
    {
        either!(self, iter => iter.gt(other))
    }

    pub fn ge<I>(self, other: I) -> bool
    where
        I: IntoParallelIterator + IntoIterator<Item = <I as IntoParallelIterator>::Item>,
        I::Iter: IndexedParallelIterator,
        P::Item: PartialOrd<<I as IntoParallelIterator>::Item>,
    {
        either!(self, iter => iter.ge(other))
    }

    pub fn enumerate(self) -> CondIterator<ri::Enumerate<P>, si::Enumerate<S>> {
        wrap_either!(self, iter => iter.enumerate())
    }

    pub fn skip(self, n: usize) -> CondIterator<ri::Skip<P>, si::Skip<S>> {
        wrap_either!(self, iter => iter.skip(n))
    }

    pub fn take(self, n: usize) -> CondIterator<ri::Take<P>, si::Take<S>> {
        wrap_either!(self, iter => iter.take(n))
    }

    pub fn position_any<Pred>(self, predicate: Pred) -> Option<usize>
    where
        Pred: Fn(P::Item) -> bool + Sync + Send,
    {
        match self.inner {
            Parallel(iter) => iter.position_any(predicate),
            Serial(mut iter) => iter.position(predicate),
        }
    }

    pub fn position_first<Pred>(self, predicate: Pred) -> Option<usize>
    where
        Pred: Fn(P::Item) -> bool + Sync + Send,
    {
        match self.inner {
            Parallel(iter) => iter.position_first(predicate),
            Serial(mut iter) => iter.position(predicate),
        }
    }
}

impl<P, S> CondIterator<P, S>
where
    P: IndexedParallelIterator,
    S: DoubleEndedIterator<Item = P::Item>,
{
    pub fn rev(self) -> CondIterator<ri::Rev<P>, si::Rev<S>> {
        wrap_either!(self, iter => iter.rev())
    }
}

impl<P, S> CondIterator<P, S>
where
    P: IndexedParallelIterator,
    S: ExactSizeIterator<Item = P::Item>,
{
    pub fn len(&self) -> usize {
        either!(self, ref iter => iter.len())
    }
}

impl<P, S> CondIterator<P, S>
where
    P: IndexedParallelIterator,
    S: ExactSizeIterator + DoubleEndedIterator<Item = P::Item>,
{
    pub fn position_last<Pred>(self, predicate: Pred) -> Option<usize>
    where
        Pred: Fn(P::Item) -> bool + Sync + Send,
    {
        match self.inner {
            Parallel(iter) => iter.position_last(predicate),
            Serial(mut iter) => iter.rposition(predicate),
        }
    }
}
