pub mod evm;
pub mod inspector;

use eyre::EyreHandler;
use std::{error::Error, future::Future};
use tracing_error::ErrorLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use yansi::Paint;

/// A custom context type for EDB specific error reporting via `eyre`
#[derive(Debug)]
struct Handler;

impl EyreHandler for Handler {
    fn debug(
        &self,
        error: &(dyn Error + 'static),
        f: &mut core::fmt::Formatter<'_>,
    ) -> core::fmt::Result {
        if f.alternate() {
            return core::fmt::Debug::fmt(error, f);
        }
        writeln!(f)?;
        write!(f, "{}", error.red())?;

        if let Some(cause) = error.source() {
            write!(f, "\n\nContext:")?;

            let multiple = cause.source().is_some();
            let errors = std::iter::successors(Some(cause), |e| (*e).source());

            for (n, error) in errors.enumerate() {
                writeln!(f)?;
                if multiple {
                    write!(f, "- Error #{n}: {error}")?;
                } else {
                    write!(f, "- {error}")?;
                }
            }
        }

        Ok(())
    }
}

/// Installs the Foundry eyre hook as the global error report hook.
///
/// # Details
///
/// By default a simple user-centric handler is installed, unless
/// `MEDGA_DEBUG` is set in the environment, in which case a more
/// verbose debug-centric handler is installed.
///
/// Panics are always caught by the more debug-centric handler.
pub fn install_error_handler() {
    // If the user has not explicitly overridden "RUST_BACKTRACE", then produce full backtraces.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::env::set_var("RUST_BACKTRACE", "full");
    }

    let debug_enabled = std::env::var("MEDGA_DEBUG").is_ok();
    if debug_enabled {
        if let Err(e) = color_eyre::install() {
            warn!("failed to install color eyre error hook: {e}");
        }
    } else {
        let (panic_hook, _) = color_eyre::config::HookBuilder::default()
            .panic_section(
                "This is a bug. Consider reporting it at https://github.com/MEDGA-eth/EDB",
            )
            .into_hooks();
        panic_hook.install();
        if let Err(e) = eyre::set_hook(Box::new(move |_| Box::new(Handler))) {
            warn!("failed to install eyre error hook: {e}");
        }
    }
}

/// Initializes a tracing Subscriber for logging
pub fn subscriber() {
    tracing_subscriber::Registry::default()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(ErrorLayer::default())
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init()
}

/// Sets the default [`yansi`] color output condition.
pub fn enable_paint() {
    let enable = yansi::Condition::os_support() && yansi::Condition::tty_and_color_live();
    yansi::whenever(yansi::Condition::cached(enable));
}

/// Runs the `future` in a new [`tokio::runtime::Runtime`]
pub fn block_on<F: Future>(future: F) -> F::Output {
    let rt = tokio::runtime::Runtime::new().expect("could not start tokio rt");
    rt.block_on(future)
}
