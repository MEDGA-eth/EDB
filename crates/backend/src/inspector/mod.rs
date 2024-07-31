mod debug;
mod push_jmp;
mod visited_address;

pub use debug::DebugInspector;
pub use push_jmp::{JumpLabel, PushJumpInspector, PushLabel};
pub use visited_address::VisitedAddrInspector;

use eyre::Result;

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
                debug_assert!(false, "{}", msg);
                T::default()
            }
        }
    }
}

impl<T> AssertionUnwrap<T> for Result<T>
where
    T: Default,
{
    fn assert_unwrap(self, msg: &str) -> T {
        match self {
            Ok(value) => value,
            Err(err) => {
                debug_assert!(false, "{}: {}", msg, err);
                T::default()
            }
        }
    }
}
