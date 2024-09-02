mod call_trace;
mod debug;
mod push_jmp;
mod visited_address;

use std::fmt::Display;

pub use call_trace::{AnalyzedCallTrace, CallTraceInspector};
pub use debug::DebugInspector;
pub use push_jmp::PushJumpInspector;
pub use visited_address::VisitedAddrInspector;

trait AssertionUnwrap<T> {
    fn assert_unwrap(self, msg: &str) -> T;
}

impl<T> AssertionUnwrap<T> for Option<T>
where
    T: Default,
{
    fn assert_unwrap(self, msg: &str) -> T {
        match self {
            Some(value) => value,
            None => {
                debug_assert!(false, "{msg}");
                T::default()
            }
        }
    }
}

impl<T, E> AssertionUnwrap<T> for Result<T, E>
where
    T: Default,
    E: Display,
{
    fn assert_unwrap(self, msg: &str) -> T {
        match self {
            Ok(value) => value,
            Err(err) => {
                debug_assert!(false, "{msg}: {err}");
                T::default()
            }
        }
    }
}
