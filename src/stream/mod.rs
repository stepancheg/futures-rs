//! Asynchronous streams
//!
//! This module contains the `Stream` trait and a number of adaptors for this
//! trait. This trait is very similar to the `Iterator` trait in the standard
//! library except that it expresses the concept of blocking as well. A stream
//! here is a sequential sequence of values which may take some amount of time
//! in between to produce.
//!
//! A stream may request that it is blocked between values while the next value
//! is calculated, and provides a way to get notified once the next value is
//! ready as well.
// TODO: expand these docs

use {IntoFuture, Poll};

mod iter;
pub use self::iter::{iter, IterStream};

mod and_then;
mod empty;
mod filter;
mod filter_map;
mod flatten;
mod fold;
mod for_each;
mod fuse;
mod future;
mod map;
mod map_err;
mod merge;
mod or_else;
mod peek;
mod skip;
mod skip_while;
mod take;
mod then;
mod zip;
pub use self::and_then::AndThen;
pub use self::empty::{Empty, empty};
pub use self::filter::Filter;
pub use self::filter_map::FilterMap;
pub use self::flatten::Flatten;
pub use self::fold::Fold;
pub use self::for_each::ForEach;
pub use self::fuse::Fuse;
pub use self::future::StreamFuture;
pub use self::map::Map;
pub use self::map_err::MapErr;
pub use self::merge::{Merge, MergedItem};
pub use self::or_else::OrElse;
pub use self::skip::Skip;
pub use self::skip_while::SkipWhile;
pub use self::take::Take;
pub use self::then::Then;
pub use self::zip::Zip;
pub use self::peek::Peekable;

if_std! {
    use std;

    mod buffered;
    mod buffer_unordered;
    mod catch_unwind;
    mod channel;
    mod collect;
    mod wait;
    pub use self::buffered::Buffered;
    pub use self::buffer_unordered::BufferUnordered;
    pub use self::catch_unwind::CatchUnwind;
    pub use self::channel::{channel, Sender, Receiver, FutureSender};
    pub use self::collect::Collect;
    pub use self::wait::Wait;

    /// A type alias for `Box<Stream + Send>`
    pub type BoxStream<T, E> = ::std::boxed::Box<Stream<Item = T, Error = E> + Send>;

    impl<S: ?Sized + Stream> Stream for ::std::boxed::Box<S> {
        type Item = S::Item;
        type Error = S::Error;

        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            (**self).poll()
        }
    }
}

/// A stream of values, not all of which have been produced yet.
///
/// `Stream` is a trait to represent any source of sequential events or items
/// which acts like an iterator but may block over time. Like `Future` the
/// methods of `Stream` never block and it is thus suitable for programming in
/// an asynchronous fashion. This trait is very similar to the `Iterator` trait
/// in the standard library where `Some` is used to signal elements of the
/// stream and `None` is used to indicate that the stream is finished.
///
/// Like futures a stream has basic combinators to transform the stream, perform
/// more work on each item, etc.
///
/// # Streams as Futures
///
/// Any instance of `Stream` can also be viewed as a `Future` where the resolved
/// value is the next item in the stream along with the rest of the stream. The
/// `into_future` adaptor can be used here to convert any stream into a future
/// for use with other future methods like `join` and `select`.
// TODO: more here
pub trait Stream {
    /// The type of item this stream will yield on success.
    type Item;

    /// The type of error this stream may generate.
    type Error;

    /// Attempt to pull out the next value of this stream, returning `None` if
    /// the stream is finished.
    ///
    /// This method, like `Future::poll`, is the sole method of pulling out a
    /// value from a stream. This method must also be run within the context of
    /// a task typically and implementors of this trait must ensure that
    /// implementations of this method do not block, as it may cause consumers
    /// to behave badly.
    ///
    /// # Return value
    ///
    /// If `NotReady` is returned then this stream's next value is not ready
    /// yet, then implementations will ensure that the current task will be
    /// notified when the next value may be ready. If `Some` is returned then
    /// the returned value represents the next value on the stream. `Err`
    /// indicates an error happened, while `Ok` indicates whether there was a
    /// new item on the stream or whether the stream has terminated.
    ///
    /// # Panics
    ///
    /// Once a stream is finished, that is `Ready(None)` has been returned,
    /// further calls to `poll` may result in a panic or other "bad behavior".
    /// If this is difficult to guard against then the `fuse` adapter can be
    /// used to ensure that `poll` always has well-defined semantics.
    // TODO: more here
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error>;

    // TODO: should there also be a method like `poll` but doesn't return an
    //       item? basically just says "please make more progress internally"
    //       seems crucial for buffering to actually make any sense.

    /// Creates an iterator which blocks the current thread until each item of
    /// this stream is resolved.
    ///
    /// This method will consume ownership of this stream, returning an
    /// implementation of a standard iterator. This iterator will *block the
    /// current thread* on each call to `next` if the item in the future isn't
    /// ready yet.
    ///
    /// > **Note:** This method is not appropriate to call on event loops or
    /// >           similar I/O situations because it will prevent the event
    /// >           loop from making progress (this blocks the thread). This
    /// >           method should only be called when it's guaranteed that the
    /// >           blocking work associated with this future will be completed
    /// >           by another thread.
    ///
    /// # Behavior
    ///
    /// This function will *pin* this stream to the thread that calls `next`.
    /// The stream will only be polled by this thread.
    ///
    /// # Panics
    ///
    /// The returned iterator does not attempt to catch panics. If the `poll`
    /// function panics, panics will be propagated to the caller of `next`.
    #[cfg(feature = "use_std")]
    fn wait(self) -> Wait<Self>
        where Self: Sized
    {
        wait::new(self)
    }

    /// Convenience function for turning this stream into a trait object.
    ///
    /// This simply avoids the need to write `Box::new` and can often help with
    /// type inference as well by always returning a trait object. Note that
    /// this method requires the `Send` bound and returns a `BoxStream`, which
    /// also encodes this. If you'd like to create a `Box<Stream>` without the
    /// `Send` bound, then the `Box::new` function can be used instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures::stream::*;
    ///
    /// let (_tx, rx) = channel();
    /// let a: BoxStream<i32, i32> = rx.boxed();
    /// ```
    #[cfg(feature = "use_std")]
    fn boxed(self) -> BoxStream<Self::Item, Self::Error>
        where Self: Sized + Send + 'static,
    {
        ::std::boxed::Box::new(self)
    }

    /// Converts this stream into a `Future`.
    ///
    /// A stream can be viewed as a future which will resolve to a pair containing
    /// the next element of the stream plus the remaining stream. If the stream
    /// terminates, then the next element is `None` and the remaining stream is
    /// still passed back, to allow reclamation of its resources.
    ///
    /// The returned future can be used to compose streams and futures together by
    /// placing everything into the "world of futures".
    fn into_future(self) -> StreamFuture<Self>
        where Self: Sized
    {
        future::new(self)
    }

    /// Converts a stream of type `T` to a stream of type `U`.
    ///
    /// The provided closure is executed over all elements of this stream as
    /// they are made available, and the callback will be executed inline with
    /// calls to `poll`.
    ///
    /// Note that this function consumes the receiving future and returns a
    /// wrapped version of it, similar to the existing `map` methods in the
    /// standard library.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures::stream::*;
    ///
    /// let (_tx, rx) = channel::<i32, u32>();
    /// let rx = rx.map(|x| x + 3);
    /// ```
    fn map<U, F>(self, f: F) -> Map<Self, F>
        where F: FnMut(Self::Item) -> U,
              Self: Sized
    {
        map::new(self, f)
    }

    /// Converts a stream of error type `T` to a stream of error type `U`.
    ///
    /// The provided closure is executed over all errors of this stream as
    /// they are made available, and the callback will be executed inline with
    /// calls to `poll`.
    ///
    /// Note that this function consumes the receiving future and returns a
    /// wrapped version of it, similar to the existing `map_err` methods in the
    /// standard library.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures::stream::*;
    ///
    /// let (_tx, rx) = channel::<i32, u32>();
    /// let rx = rx.map_err(|x| x + 3);
    /// ```
    fn map_err<U, F>(self, f: F) -> MapErr<Self, F>
        where F: FnMut(Self::Error) -> U,
              Self: Sized
    {
        map_err::new(self, f)
    }

    /// Filters the values produced by this stream according to the provided
    /// predicate.
    ///
    /// As values of this stream are made available, the provided predicate will
    /// be run against them. If the predicate returns `true` then the stream
    /// will yield the value, but if the predicate returns `false` then the
    /// value will be discarded and the next value will be produced.
    ///
    /// All errors are passed through without filtering in this combinator.
    ///
    /// Note that this function consumes the receiving future and returns a
    /// wrapped version of it, similar to the existing `filter` methods in the
    /// standard library.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures::stream::*;
    ///
    /// let (_tx, rx) = channel::<i32, u32>();
    /// let evens = rx.filter(|x| x % 0 == 2);
    /// ```
    fn filter<F>(self, f: F) -> Filter<Self, F>
        where F: FnMut(&Self::Item) -> bool,
              Self: Sized
    {
        filter::new(self, f)
    }

    /// Filters the values produced by this stream while simultaneously mapping
    /// them to a different type.
    ///
    /// As values of this stream are made available, the provided function will
    /// be run on them. If the predicate returns `Some(e)` then the stream will
    /// yield the value `e`, but if the predicate returns `None` then the next
    /// value will be produced.
    ///
    /// All errors are passed through without filtering in this combinator.
    ///
    /// Note that this function consumes the receiving future and returns a
    /// wrapped version of it, similar to the existing `filter_map` methods in the
    /// standard library.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures::stream::*;
    ///
    /// let (_tx, rx) = channel::<i32, u32>();
    /// let evens_plus_one = rx.filter_map(|x| {
    ///     if x % 0 == 2 {
    ///         Some(x + 1)
    ///     } else {
    ///         None
    ///     }
    /// });
    /// ```
    fn filter_map<F, B>(self, f: F) -> FilterMap<Self, F>
        where F: FnMut(Self::Item) -> Option<B>,
              Self: Sized
    {
        filter_map::new(self, f)
    }

    /// Chain on a computation for when a value is ready, passing the resulting
    /// item to the provided closure `f`.
    ///
    /// This function can be used to ensure a computation runs regardless of
    /// the next value on the stream. The closure provided will be yielded a
    /// `Result` once a value is ready, and the returned future will then be run
    /// to completion to produce the next value on this stream.
    ///
    /// The returned value of the closure must implement the `IntoFuture` trait
    /// and can represent some more work to be done before the composed stream
    /// is finished. Note that the `Result` type implements the `IntoFuture`
    /// trait so it is possible to simply alter the `Result` yielded to the
    /// closure and return it.
    ///
    /// Note that this function consumes the receiving future and returns a
    /// wrapped version of it.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures::stream::*;
    ///
    /// let (_tx, rx) = channel::<i32, u32>();
    ///
    /// let rx = rx.then(|result| {
    ///     match result {
    ///         Ok(e) => Ok(e + 3),
    ///         Err(e) => Err(e - 4),
    ///     }
    /// });
    /// ```
    fn then<F, U>(self, f: F) -> Then<Self, F, U>
        where F: FnMut(Result<Self::Item, Self::Error>) -> U,
              U: IntoFuture,
              Self: Sized
    {
        then::new(self, f)
    }

    /// Chain on a computation for when a value is ready, passing the successful
    /// results to the provided closure `f`.
    ///
    /// This function can be used to run a unit of work when the next successful
    /// value on a stream is ready. The closure provided will be yielded a value
    /// when ready, and the returned future will then be run to completion to
    /// produce the next value on this stream.
    ///
    /// Any errors produced by this stream will not be passed to the closure,
    /// and will be passed through.
    ///
    /// The returned value of the closure must implement the `IntoFuture` trait
    /// and can represent some more work to be done before the composed stream
    /// is finished. Note that the `Result` type implements the `IntoFuture`
    /// trait so it is possible to simply alter the `Result` yielded to the
    /// closure and return it.
    ///
    /// Note that this function consumes the receiving future and returns a
    /// wrapped version of it.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures::stream::*;
    ///
    /// let (_tx, rx) = channel::<i32, u32>();
    ///
    /// let rx = rx.and_then(|result| {
    ///     if result % 2 == 0 {
    ///         Ok(result)
    ///     } else {
    ///         Err(result as u32)
    ///     }
    /// });
    /// ```
    fn and_then<F, U>(self, f: F) -> AndThen<Self, F, U>
        where F: FnMut(Self::Item) -> U,
              U: IntoFuture<Error = Self::Error>,
              Self: Sized
    {
        and_then::new(self, f)
    }

    /// Chain on a computation for when an error happens, passing the
    /// erroneous result to the provided closure `f`.
    ///
    /// This function can be used to run a unit of work and attempt to recover from
    /// an error if one happens. The closure provided will be yielded an error
    /// when one appears, and the returned future will then be run to completion
    /// to produce the next value on this stream.
    ///
    /// Any successful values produced by this stream will not be passed to the
    /// closure, and will be passed through.
    ///
    /// The returned value of the closure must implement the `IntoFuture` trait
    /// and can represent some more work to be done before the composed stream
    /// is finished. Note that the `Result` type implements the `IntoFuture`
    /// trait so it is possible to simply alter the `Result` yielded to the
    /// closure and return it.
    ///
    /// Note that this function consumes the receiving future and returns a
    /// wrapped version of it.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures::stream::*;
    ///
    /// let (_tx, rx) = channel::<i32, u32>();
    ///
    /// let rx = rx.or_else(|result| {
    ///     if result % 2 == 0 {
    ///         Ok(result as i32)
    ///     } else {
    ///         Err(result)
    ///     }
    /// });
    /// ```
    fn or_else<F, U>(self, f: F) -> OrElse<Self, F, U>
        where F: FnMut(Self::Error) -> U,
              U: IntoFuture<Item = Self::Item>,
              Self: Sized
    {
        or_else::new(self, f)
    }

    /// Collect all of the values of this stream into a vector, returning a
    /// future representing the result of that computation.
    ///
    /// This combinator will collect all successful results of this stream and
    /// collect them into a `Vec<Self::Item>`. If an error happens then all
    /// collected elements will be dropped and the error will be returned.
    ///
    /// The returned future will be resolved whenever an error happens or when
    /// the stream returns `Ok(None)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::thread;
    /// use futures::{finished, Future, Poll, BoxFuture};
    /// use futures::stream::*;
    ///
    /// let (tx, rx) = channel::<i32, u32>();
    ///
    /// fn send(n: i32, tx: Sender<i32, u32>) -> BoxFuture<(), ()> {
    ///     if n == 0 {
    ///         return finished(()).boxed()
    ///     }
    ///     tx.send(Ok(n)).map_err(|_| ()).and_then(move |tx| {
    ///         send(n - 1, tx)
    ///     }).boxed()
    /// }
    ///
    /// thread::spawn(|| send(5, tx).wait());
    ///
    /// let mut result = rx.collect();
    /// assert_eq!(result.wait(), Ok(vec![5, 4, 3, 2, 1]));
    /// ```
    #[cfg(feature = "use_std")]
    fn collect(self) -> Collect<Self>
        where Self: Sized
    {
        collect::new(self)
    }

    /// Execute an accumulating computation over a stream, collecting all the
    /// values into one final result.
    ///
    /// This combinator will collect all successful results of this stream
    /// according to the closure provided. The initial state is also provided to
    /// this method and then is returned again by each execution of the closure.
    /// Once the entire stream has been exhausted the returned future will
    /// resolve to this value.
    ///
    /// If an error happens then collected state will be dropped and the error
    /// will be returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::thread;
    /// use futures::{finished, Future, Poll, BoxFuture};
    /// use futures::stream::*;
    ///
    /// let (tx, rx) = channel::<i32, u32>();
    ///
    /// fn send(n: i32, tx: Sender<i32, u32>) -> BoxFuture<(), ()> {
    ///     if n == 0 {
    ///         return finished(()).boxed()
    ///     }
    ///     tx.send(Ok(n)).map_err(|_| ()).and_then(move |tx| {
    ///         send(n - 1, tx)
    ///     }).boxed()
    /// }
    ///
    /// thread::spawn(|| send(5, tx).wait());
    ///
    /// let mut result = rx.fold(0, |a, b| finished::<i32, u32>(a + b));
    /// assert_eq!(result.wait(), Ok(15));
    /// ```
    fn fold<F, T, Fut>(self, init: T, f: F) -> Fold<Self, F, Fut, T>
        where F: FnMut(T, Self::Item) -> Fut,
              Fut: IntoFuture<Item = T>,
              Self::Error: From<Fut::Error>,
              Self: Sized
    {
        fold::new(self, f, init)
    }

    /// Flattens a stream of streams into just one continuous stream.
    ///
    /// If this stream's elements are themselves streams then this combinator
    /// will flatten out the entire stream to one long chain of elements. Any
    /// errors are passed through without looking at them, but otherwise each
    /// individual stream will get exhausted before moving on to the next.
    ///
    /// ```
    /// use std::thread;
    /// use futures::{finished, Future, Poll};
    /// use futures::stream::*;
    ///
    /// let (tx1, rx1) = channel::<i32, u32>();
    /// let (tx2, rx2) = channel::<i32, u32>();
    /// let (tx3, rx3) = channel::<_, u32>();
    ///
    /// thread::spawn(|| tx1.send(Ok(1)).and_then(|tx1| tx1.send(Ok(2))).wait());
    /// thread::spawn(|| tx2.send(Ok(3)).and_then(|tx2| tx2.send(Ok(4))).wait());
    ///
    /// thread::spawn(|| tx3.send(Ok(rx1)).and_then(|tx3| tx3.send(Ok(rx2))).wait());
    ///
    /// let mut result = rx3.flatten().collect();
    /// assert_eq!(result.wait(), Ok(vec![1, 2, 3, 4]));
    /// ```
    fn flatten(self) -> Flatten<Self>
        where Self::Item: Stream,
              <Self::Item as Stream>::Error: From<Self::Error>,
              Self: Sized
    {
        flatten::new(self)
    }

    /// Skip elements on this stream while the predicate provided resolves to
    /// `true`.
    ///
    /// This function, like `Iterator::skip_while`, will skip elements on the
    /// stream until the `predicate` resolves to `false`. Once one element
    /// returns false all future elements will be returned from the underlying
    /// stream.
    fn skip_while<P, R>(self, pred: P) -> SkipWhile<Self, P, R>
        where P: FnMut(&Self::Item) -> R,
              R: IntoFuture<Item=bool, Error=Self::Error>,
              Self: Sized
    {
        skip_while::new(self, pred)
    }

    /// Runs this stream to completion, executing the provided closure for each
    /// element on the stream.
    ///
    /// The closure provided will be called for each item this stream resolves
    /// to successfully, and the closure can optionally fail by returning a
    /// `Result`.
    ///
    /// The returned value is a `Future` where the `Item` type is `()` and
    /// errors are otherwise threaded through. Any error on the stream or in the
    /// closure will cause iteration to be halted immediately and the future
    /// will resolve to that error.
    fn for_each<F>(self, f: F) -> ForEach<Self, F>
        where F: FnMut(Self::Item) -> Result<(), Self::Error>,
              Self: Sized
    {
        for_each::new(self, f)
    }

    /// Creates a new stream of at most `amt` items.
    ///
    /// Once `amt` items have been yielded from this stream then it will always
    /// return that the stream is done.
    fn take(self, amt: u64) -> Take<Self>
        where Self: Sized
    {
        take::new(self, amt)
    }

    /// Creates a new stream which skips `amt` items of the underlying stream.
    ///
    /// Once `amt` items have been skipped from this stream then it will always
    /// return the remaining items on this stream.
    fn skip(self, amt: u64) -> Skip<Self>
        where Self: Sized
    {
        skip::new(self, amt)
    }

    /// Fuse a stream such that `poll`/`schedule` will never again be called
    /// once it has terminated (signaled emptyness or an error).
    ///
    /// Currently once a stream has returned `Some(Ok(None))` from `poll` any further
    /// calls could exhibit bad behavior such as block forever, panic, never
    /// return, etc. If it is known that `poll` may be called too often then
    /// this method can be used to ensure that it has defined semantics.
    ///
    /// Once a stream has been `fuse`d and it terminates, then
    /// it will forever return `None` from `poll` again (never resolve). This,
    /// unlike the trait's `poll` method, is guaranteed.
    ///
    /// Additionally, once a stream has completed, this `Fuse` combinator will
    /// never call `schedule` on the underlying stream.
    fn fuse(self) -> Fuse<Self>
        where Self: Sized
    {
        fuse::new(self)
    }

    /// Catches unwinding panics while polling the future.
    ///
    /// In general, panics within a future can propagate all the way out to the
    /// task level. This combinator makes it possible to halt unwinding within
    /// the future itself. It's most commonly used within task executors.
    ///
    /// Note that this method requires the `UnwindSafe` bound from the standard
    /// library. This isn't always applied automatically, and the standard
    /// library provides an `AssertUnwindSafe` wrapper type to apply it
    /// after-the fact. To assist using this method, the `Future` trait is also
    /// implemented for `AssertUnwindSafe<F>` where `F` implements `Future`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use futures::stream;
    /// use futures::stream::Stream;
    ///
    /// let stream = stream::iter::<_, Option<i32>, bool>(vec![Some(10), None].into_iter().map(Ok));
    /// // panic on second element
    /// let stream_panicking = stream.map(|o| o.unwrap());
    /// let mut iter = stream_panicking.catch_unwind().wait();
    ///
    /// assert_eq!(Ok(10), iter.next().unwrap().ok().unwrap());
    /// assert!(iter.next().unwrap().is_err());
    /// assert!(iter.next().is_none());
    /// ```
    #[cfg(feature = "use_std")]
    fn catch_unwind(self) -> CatchUnwind<Self>
        where Self: Sized + std::panic::UnwindSafe
    {
        catch_unwind::new(self)
    }

    /// An adaptor for creating a buffered list of pending futures.
    ///
    /// If this stream's item can be converted into a future, then this adaptor
    /// will buffer up to `amt` futures and then return results in the same
    /// order as the underlying stream. No more than `amt` futures will be
    /// buffered at any point in time, and less than `amt` may also be buffered
    /// depending on the state of each future.
    ///
    /// The returned stream will be a stream of each future's result, with
    /// errors passed through whenever they occur.
    #[cfg(feature = "use_std")]
    fn buffered(self, amt: usize) -> Buffered<Self>
        where Self::Item: IntoFuture<Error = <Self as Stream>::Error>,
              Self: Sized
    {
        buffered::new(self, amt)
    }

    /// An adaptor for creating a buffered list of pending futures (unordered).
    ///
    /// If this stream's item can be converted into a future, then this adaptor
    /// will buffer up to `amt` futures and then return results in the order
    /// in which they complete. No more than `amt` futures will be buffered at
    /// any point in time, and less than `amt` may also be buffered depending on
    /// the state of each future.
    ///
    /// The returned stream will be a stream of each future's result, with
    /// errors passed through whenever they occur.
    #[cfg(feature = "use_std")]
    fn buffer_unordered(self, amt: usize) -> BufferUnordered<Self>
        where Self::Item: IntoFuture<Error = <Self as Stream>::Error>,
              Self: Sized
    {
        buffer_unordered::new(self, amt)
    }

    /// An adapter for merging the output of two streams.
    ///
    /// The merged stream produces items from one or both of the underlying
    /// streams as they become available. Errors, however, are not merged: you
    /// get at most one error at a time.
    fn merge<S>(self, other: S) -> Merge<Self, S>
        where S: Stream<Error = Self::Error>,
              Self: Sized,
    {
        merge::new(self, other)
    }

    /// An adapter for zipping two streams together.
    ///
    /// The zipped stream waits for both streams to produce an item, and then
    /// returns that pair. If an error happens, then that error will be returned
    /// immediately. If either stream ends then the zipped stream will also end.
    fn zip<S>(self, other: S) -> Zip<Self, S>
        where S: Stream<Error = Self::Error>,
              Self: Sized,
    {
        zip::new(self, other)
    }

    /// Creates a new stream witch exposes a `peek` method.
    ///
    /// Calling `peek` returns a reference to the next item in the stream.
    fn peekable(self) -> Peekable<Self>
        where Self: Sized
    {
        peek::new(self)
    }
}

impl<'a, S: ?Sized + Stream> Stream for &'a mut S {
    type Item = S::Item;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        (**self).poll()
    }
}
