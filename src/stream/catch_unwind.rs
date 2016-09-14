use std::prelude::v1::*;
use std::any::Any;
use std::panic::{catch_unwind, UnwindSafe, AssertUnwindSafe};

use super::super::{Poll, Async};
use super::Stream;

/// Stream for the `catch_unwind` combinator.
///
/// This is created by this `Stream::catch_unwind` method.
#[must_use = "streams do nothing unless polled"]
pub struct CatchUnwind<S> where S: Stream {
    stream: S,
}

pub fn new<S>(stream: S) -> CatchUnwind<S>
    where S: Stream + UnwindSafe,
{
    CatchUnwind {
        stream: stream,
    }
}

impl<S> Stream for CatchUnwind<S>
    where S: Stream + UnwindSafe,
{
    type Item = Result<S::Item, S::Error>;
    type Error = Box<Any + Send>;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let res = try!(catch_unwind(AssertUnwindSafe(|| self.stream.poll())));
        match res {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(t)) => Ok(Async::Ready(t.map(Ok))),
            Err(e) => Ok(Async::Ready(Some(Err(e)))),
        }
    }
}

impl<S: Stream> Stream for AssertUnwindSafe<S> {
    type Item = S::Item;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<S::Item>, S::Error> {
        self.0.poll()
    }
}
